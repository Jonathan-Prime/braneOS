# ============================================================
# Brane OS — Build System
# ============================================================

KERNEL_BIN := target/x86_64-unknown-none/debug/brane_os_kernel
KERNEL_RELEASE := target/x86_64-unknown-none/release/brane_os_kernel
BUILD_FLAGS := -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem --target x86_64-unknown-none

.PHONY: build build-release run run-release test fmt clippy clean help

# --- Build -------------------------------------------------------------------

build: ## Build kernel (debug)
	cd kernel && cargo build $(BUILD_FLAGS)

build-release: ## Build kernel (release, with LTO)
	cd kernel && cargo build --release $(BUILD_FLAGS)

# --- Run in QEMU -------------------------------------------------------------

run: build ## Build and run in QEMU (debug)
	KERNEL_BIN_PATH=$(KERNEL_BIN) cargo run --package runner

run-release: build-release ## Build and run in QEMU (release)
	KERNEL_BIN_PATH=$(KERNEL_RELEASE) cargo run --package runner --release

# --- Quality -----------------------------------------------------------------

fmt: ## Format all Rust code
	cargo fmt --all

clippy: ## Run Clippy lints
	cd kernel && cargo clippy $(BUILD_FLAGS) -- -D warnings
	cd runner && cargo clippy --all-targets -- -D warnings

test: ## Run unit tests (host-side)
	cd kernel && cargo test --lib

# --- Housekeeping ------------------------------------------------------------

clean: ## Remove build artifacts
	cargo clean
	rm -f *.img *.iso *.bin

# --- Help --------------------------------------------------------------------

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
