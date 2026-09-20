//! Native application runtime.
//!
//! [`NativeRuntime`] implements [`ferrix_app_runtime_api::Runtime`] for
//! [`ferrix_app_api::RuntimeKind::Native`]: every application becomes its own
//! supervised process, started through the `ferrix-sandbox` binary shipped by
//! this crate, which puts it into Linux namespaces according to the
//! manifest's runtime-private [`IsolationPolicy`].
//!
//! Process tree of one launch:
//!
//! ```text
//! ferrix_appd -> ferrix-sandbox (stage 1, unshares) -> ferrix-sandbox --init (PID 1 of the
//!         new PID namespace, mounts, reaps, forwards signals) -> application
//! ```
//!
//! The library is platform independent: it builds and tests on macOS, where
//! the sandbox is reported as unavailable. Everything that touches namespaces
//! lives in the binary and in the `cfg(target_os = "linux")` parts of
//! [`report`].
//!
//! Nothing here depends on QEMU, kernels, or boards; `ferrix_appd` composes this
//! runtime in its bootstrap code only.

mod plan;
mod policy;
mod report;
mod runtime;
mod sockets;

pub use plan::*;
pub use policy::*;
pub use report::*;
pub use runtime::*;
pub use sockets::*;
