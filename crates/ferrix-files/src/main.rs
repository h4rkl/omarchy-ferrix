//! Omarchy Ferrix Files: an independent Wayland client that browses the
//! user's home directory. Needs no platform daemon and no capability grant:
//! `$HOME` is the only filesystem it reads or writes, and every path is
//! confined to it (see `store`).
//!
//! Usage:
//!   ferrix-files                                     # guest/nested: Wayland
//!   ferrix-files --preview out.ppm [--page NAME] [--size WxH]
//!
//! Pages: `browse` (default), `view`, `item`, `new-folder`, `rename`,
//! `confirm-delete`, plus the preview-only `empty`. Preview renders a
//! synthetic home in a scratch folder (mock evidence), never the real one.
//! Records: `app-context`, `files-frame`, `ui-state {page, state}`,
//! `files-action {action, status, error}`, `files-back {status}`; depths,
//! counts, and error codes only, never file names or paths.

mod interaction;
mod model;
mod screen;
mod scroll;
mod store;

#[cfg(target_os = "linux")]
mod app;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ferrix_ui::draw::Type;
use ferrix_ui::evidence::{SessionEvidence, emit, json_escape, json_opt_str};
use ferrix_ui::theme;
use jiff::tz::TimeZone;

use crate::model::Model;
use crate::store::Home;

const EVIDENCE: SessionEvidence = SessionEvidence;
const PREVIEW_SIZE: (u32, u32) = (1080, 2340);
pub const APP_ID: &str = "org.ferrix.files";

fn app_context_record(evidence: &str) -> String {
    let app_id = std::env::var(ferrix_app_api::APP_ID_ENV)
        .ok()
        .filter(|v| !v.is_empty());
    let instance = std::env::var(ferrix_app_api::APP_INSTANCE_ENV)
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    format!(
        "{{\"evidence\":\"{evidence}\",\"event\":\"app-context\",\"app\":\"{APP_ID}\",\"app_id\":{},\"instance\":{}}}",
        json_opt_str(app_id.as_deref()),
        instance.map_or_else(|| "null".to_owned(), |n| n.to_string())
    )
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or_else(|| format!("--size expects WxH, got {text:?}"))?;
    match (w.parse::<u32>(), h.parse::<u32>()) {
        (Ok(w), Ok(h)) if w > 0 && h > 0 => Ok((w, h)),
        _ => Err(format!("--size expects WxH, got {text:?}")),
    }
}

/// A synthetic home for previews: the XDG folders, a Contacts-style vCard
/// export with fixture data, and a note. Fixed timestamps keep frames
/// reproducible.
fn write_preview_home(root: &Path) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    let _ = std::fs::remove_dir_all(root);
    for dir in ["Documents", "Downloads", "Music", "Pictures", "Videos"] {
        std::fs::create_dir_all(root.join(dir)).map_err(io)?;
    }
    let vcard: String = (1..=3)
        .map(|i| {
            format!(
                "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Test Contact {i:02}\r\nTEL;TYPE=cell:+1555010{i:04}\r\nEND:VCARD\r\n"
            )
        })
        .collect();
    std::fs::write(root.join("Documents/contacts.vcf"), vcard).map_err(io)?;
    std::fs::write(
        root.join("Documents/notes.txt"),
        "Files preview fixture.\n\nThis text is synthetic mock data.\n",
    )
    .map_err(io)?;
    std::fs::write(root.join("Downloads/readme.txt"), "fixture\n").map_err(io)?;
    // 2026-10-09 09:41 UTC for every entry, folders last so writing their
    // children does not move them again.
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_791_538_860);
    for path in [
        "Documents/contacts.vcf",
        "Documents/notes.txt",
        "Downloads/readme.txt",
        "Documents",
        "Downloads",
        "Music",
        "Pictures",
        "Videos",
    ] {
        std::fs::File::options()
            .read(true)
            .open(root.join(path))
            .and_then(|f| f.set_modified(at))
            .map_err(io)?;
    }
    Ok(())
}

fn preview_model(home: &Path, page: &str) -> Result<Model, String> {
    let mut model = Model::new(Home::new(home), TimeZone::UTC);
    let enter_documents = |m: &mut Model| {
        m.open("Documents");
    };
    match page {
        "browse" => {}
        "empty" => model.open("Music"),
        "view" => {
            enter_documents(&mut model);
            model.open("contacts.vcf");
        }
        "item" | "rename" | "confirm-delete" => {
            enter_documents(&mut model);
            let index = model
                .entries()
                .iter()
                .position(|e| e.name == "contacts.vcf")
                .ok_or("preview fixture is missing contacts.vcf")?;
            model.inspect(index);
            match page {
                "rename" => model.rename_selected(),
                "confirm-delete" => model.ask_delete(),
                _ => {}
            }
        }
        "new-folder" => {
            model.new_folder();
            model.form = "Projects".to_owned();
        }
        other => {
            return Err(format!(
                "unknown --page {other:?}; use browse, view, item, new-folder, rename, confirm-delete, empty"
            ));
        }
    }
    Ok(model)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "{{\"evidence\":\"{EVIDENCE}\",\"event\":\"files-error\",\"error\":\"{}\"}}",
                json_escape(&err)
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut preview = None;
    let mut page = "browse".to_owned();
    let mut size = PREVIEW_SIZE;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => preview = Some(it.next().ok_or("--preview requires an output path")?),
            "--page" => page = it.next().ok_or("--page requires a page name")?,
            "--size" => size = parse_size(&it.next().ok_or("--size requires WxH")?)?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if let Some(path) = preview {
        theme::init("mock", None);
        emit(app_context_record("mock"));
        let home =
            std::env::temp_dir().join(format!("ferrix-files-preview-{}", std::process::id()));
        let rendered =
            write_preview_home(&home).and_then(|()| render_preview(&home, &page, size, &path));
        let _ = std::fs::remove_dir_all(&home);
        return rendered;
    }
    emit(app_context_record(&EVIDENCE));
    theme::init(&EVIDENCE, None);
    #[cfg(target_os = "linux")]
    {
        app::run(size)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("the Wayland session is Linux-only on this host; use --preview <out.ppm>".to_owned())
    }
}

fn render_preview(home: &Path, page: &str, size: (u32, u32), path: &str) -> Result<(), String> {
    let ty = Type::load()?;
    let model = preview_model(home, page)?;
    let canvas = screen_preview(&ty, size, &model);
    if let Some(parent) = PathBuf::from(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, canvas.ppm_bytes()).map_err(|e| e.to_string())?;
    emit(format!(
        "{{\"evidence\":\"mock\",\"event\":\"files-preview\",\"page\":\"{}\",\"state\":\"{}\",\"path\":\"{}\",\"width\":{},\"height\":{}}}",
        model.page.name(),
        json_escape(&model.state_name()),
        json_escape(path),
        size.0,
        size.1
    ));
    Ok(())
}

fn screen_preview(ty: &Type, size: (u32, u32), model: &Model) -> ferrix_ui::draw::Canvas {
    screen::compose(
        ty,
        size,
        ferrix_ui::clock::local_minute_of_day(),
        model,
        &interaction::Input::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Page;
    use crate::store::testdir::TestDir;

    #[test]
    fn preview_pages_render_from_the_synthetic_home() {
        let dir = TestDir::new();
        let home = dir.path().join("home");
        write_preview_home(&home).unwrap();
        let ty = Type::load().unwrap();
        for (name, page) in [
            ("browse", Page::Browse),
            ("empty", Page::Browse),
            ("view", Page::View),
            ("item", Page::Item),
            ("new-folder", Page::NewFolder),
            ("rename", Page::Rename),
            ("confirm-delete", Page::ConfirmDelete),
        ] {
            let model = preview_model(&home, name).unwrap();
            assert_eq!(model.page, page, "{name}");
            let c = screen_preview(&ty, PREVIEW_SIZE, &model);
            assert_eq!((c.width(), c.height()), (1080, 2340));
            assert!(c.ppm_bytes().starts_with(b"P6\n1080 2340\n255\n"));
        }
        let browse = preview_model(&home, "browse").unwrap();
        assert_eq!(browse.entries().len(), 5);
        assert_eq!(
            browse.date(browse.entries()[0].modified),
            "2026-10-09 09:41"
        );
        assert!(preview_model(&home, "nope").is_err());
        assert!(parse_size("x").is_err());
        assert_eq!(parse_size("720x1560"), Ok((720, 1560)));
    }
}
