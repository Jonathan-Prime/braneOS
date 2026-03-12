# Brane OS

> **Brane OS** is a custom, modular, secure, and extensible operating system designed to integrate an Artificial Intelligence layer controlled by policies, capabilities, and strict auditing.

| Status | Version | Architecture | Primary Language |
|--------|---------|--------------|------------------|
| MVP Development | `v0.1.0` | `x86_64` | Rust |

---

## 🚀 Vision

The goal is not just to build a kernel, but a complete platform featuring:
- A reliable, small, **hybrid modular kernel**.
- Decoupled system services in user space.
- A **capability-based security model**.
- Comprehensive observability and auditing.
- An **AI Subsystem** capable of observing, analyzing, suggesting, and executing restricted actions under strict control.

---

## 🧠 Core Features

### 1. Adaptability (Brane)
The OS acts as an intelligent **membrane** — a *brane* — that dynamically adapts its behavior, resource allocation, and service topology based on workload, context, and environment. Modules can be loaded, unloaded, and reconfigured at runtime without rebooting.

### 2. External Device Connection (External Branes)
Brane OS treats every connected device as an **external brane** — a peer membrane with its own capabilities. Through a secure discovery and pairing protocol, devices can share resources, delegate tasks, and form **brane clusters** for distributed computing, all mediated by the capability broker and policy engine.

### 3. Mobile Integration
First-class support for mobile device integration. Phones and tablets can act as **companion branes**, enabling:
- Remote system monitoring and control.
- Notification forwarding and AI alert delivery.
- Secure file and context sharing via the brane protocol.

### 4. AI Integration
A native AI subsystem that operates under strict capability-based security:
- **Observe** system telemetry and detect anomalies.
- **Suggest** optimizations and incident responses.
- **Execute** restricted, reversible actions when authorized by policy.
- All AI actions are fully auditable and revocable.

---

For detailed technical documentation, see the [`docs/`](docs/) directory:
- [PROJECT_MASTER_SPEC.md](docs/PROJECT_MASTER_SPEC.md)
- [ARCHITECTURE.md](docs/ARCHITECTURE.md)
- [SECURITY_MODEL.md](docs/SECURITY_MODEL.md)
- [AI_SUBSYSTEM.md](docs/AI_SUBSYSTEM.md)

---

## 🛠 Prerequisites

To build and run Brane OS locally, you'll need the following tools:

- **Rust Nightly** (`rustup default nightly`)
- Rust components: `rust-src`, `llvm-tools-preview`
- **QEMU** (`qemu-system-x86_64`) for emulation
- `make` or `just`

### macOS Setup (Homebrew)
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install nightly
rustup component add rust-src llvm-tools-preview --toolchain nightly

# Install QEMU
brew install qemu
```

---

## 🏗 Build & Run

The project includes a `Makefile` to simplify builds and testing.

```bash
# Build the kernel binary (debug)
make build

# Build the kernel binary (release, with LTO)
make build-release

# Build and launch QEMU
make run

# Run formatting and linting
make fmt && make clippy
```

---

## 📁 Repository Structure

Based on §20 of the master specification:

- `boot/` — Bootloader and early initialization
- `kernel/` — Core kernel (scheduler, memory manager, syscalls, IPC)
- `services/` — System services (init, process manager, capability broker, policy engine)
- `drivers/` — Hardware drivers (serial, timer, disk, input, net)
- `userland/` — Shell and admin tools
- `ai/` — AI Subsystem (context collector, model runtime, decision planner)
- `tests/` — Multi-level testing strategy
- `tools/` — QEMU runners and build tools

---

## 🛡 Security & AI Rules

Any AI agent or human contributor working on Brane OS must follow these core principles:
1. **Security before automation.**
2. **The kernel must remain small and maintainable.**
3. **The AI does not have direct, free access to the system.** Every sensitive action must pass through the `capability_broker`, `policy_engine`, and `audit_service`.
4. **All relevant actions are auditable.**

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
