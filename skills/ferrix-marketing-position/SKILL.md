# Project Specification: Omarchy Ferrix

## 1. Executive Summary & Vision

**Omarchy Ferrix** is a custom, Rust-first mobile operating system designed for a Linux environment. It acts as the hardened, pocket-sized companion to the **Omarchy** desktop ecosystem.

While existing mobile platforms operate as corporate, data-harvesting silos, Ferrix extends the Omarchy philosophy—**agentic design, premium minimalism, and deep developer control**—into a mobile form factor. It addresses critical community gaps by using a 100% memory-safe Rust architecture to deliver an industrial-grade, zero-bloat, secure handset engine.

## 2. Core Branding & Naming Philosophy

The name **Ferrix** represents a dual-layered branding identity that aligns with the ecosystem's aesthetic:

* **The Material Layer:** Derived from *Ferrum* (Iron), it is a direct nod to the **Rust-first** foundation and ironclad, compile-time security.

* **The Narrative Layer:** Evokes a gritty, industrial, and hyper-custom culture of self-reliance and resistance to corporate tech monopolies (reminiscent of the salvage-and-rebuild world of *Ferrix*).

### Sub-Product Positioning Matrix

* **Omarchy M:** The sleek desktop monogram for Apple Silicon.

* **Omarchy Dragon:** The high-performance engine for Qualcomm Snapdragon.

* **Omarchy Ferrix:** The hardened, memory-safe handset runtime for mobile hardware.

## 3. Product Pillars & AI Alignment

| Pillar | Architectural Implementation | Marketing Angle |
| :--- | :--- | :--- |
| **Rust-First Architecture** | Core system utilities, daemon services, and low-level device drivers written natively in Rust. | **Zero-Bloat Speed:** Rejects heavy, battery-draining Java/Android runtimes in favour of bare-metal efficiency. |
| **Agentic by Default** | Deep integration with local, lightweight LLMs and automated workflows. Syncs desktop AI tools seamlessly. | **Pocket Agents:** Your personal AI configurations and tools travel with you, running locally without cloud telemetry. |
| **Converged Ecosystem** | Zero-configuration local syncing for a unified clipboard, terminal histories, and secure keys. | **Seamless Desktop Companion:** Not an isolated mobile island, but an extension of your primary workstation. |
| **Ironclad Security** | Memory safety baked into the OS core, solving common community security criticisms. | **The Hardened Handset:** Protects your keys, data, and agent communication logs with Rust's strict safety guarantees. |

## 4. Design & User Experience (UX)

Ferrix rejects standard, hand-holding app grids. The user interface prioritizes utility, speed, and clean typography.

* **The Malleable Interface:** Designed around micro-tiling window layouts, gestural commands, and terminal-first navigation optimized for mobile screens.

* **Unified Theming (`colors.toml`):** Natively parses the desktop's system themes. Updating your workspace profile instantly updates the entire mobile shell's visual landscape.

* **Visual Palette:** Dark slate grays, industrial steel, and sharp oxidized orange (rust) accents.

## 5. Development Intent & LLM Prompting Directives

When generating code, architecture designs, or UI mockups for this project, always adhere to the following constraints:

1. **Prioritize Rust Idioms:** Avoid relying on heavy C-bindings or runtime abstractions where safe, high-performance Rust can be used.

2. **Design for Mobile Linux Constraints:** Target a generic mobile Linux environment (e.g., compatibility with Wayland/wlroots-based compositor architectures, systemd or openrc services, and low-memory footprints).

3. **Incorporate AI-Agent Architecture:** Treat automated agent tasks as first-class citizens in system design (e.g., designing secure sandboxes or system-level APIs for agent automation).

4. **Maintain the "Vibe":** Keep documentation, terminal logs, and system messages concise, highly aesthetic, and professional. Avoid corporate speak.
