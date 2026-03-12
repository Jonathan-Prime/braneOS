# ============================================================
# Brane OS — Build System
# ============================================================

KERNEL_BIN := target/x86_64-unknown-none/debug/brane_os_kernel
KERNEL_RELEASE := target/x86_64-unknown-none/release/brane_os_kernel

.PHONY: build build-release run run-release test fmt clippy clean help

# --- Build -------------------------------------------------------------------

build: ## Build kernel (debug)
	cargo build

build-release: ## Build kernel (release, with LTO)
	cargo build --release

# --- Run in QEMU -------------------------------------------------------------

run: build ## Build and run in QEMU (debug)
	@./tools/qemu_runner/run.sh $(KERNEL_BIN)

run-release: build-release ## Build and run in QEMU (release)
	@./tools/qemu_runner/run.sh $(KERNEL_RELEASE)

# --- Quality -----------------------------------------------------------------

fmt: ## Format all Rust code
	cargo fmt --all

clippy: ## Run Clippy lints
	cargo clippy --all-targets -- -D warnings

test: ## Run unit tests (host-side)
	cargo test --target x86_64-apple-darwin

# --- Housekeeping ------------------------------------------------------------

clean: ## Remove build artifacts
	cargo clean
	rm -f *.img *.iso *.bin

# --- Help --------------------------------------------------------------------

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
