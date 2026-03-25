.PHONY: specs build test check install clean

# Compile withfig specs to JSON (requires Node.js)
specs:
	node scripts/compile-specs.mjs --top 50 --out specs

# Build the tabra binary
build:
	cargo build --release

# Run all tests
test:
	cargo test --all-targets

# Type check + clippy
check:
	cargo check --all-targets
	cargo clippy --all-targets -- -D warnings

# Validate compiled specs against Rust types
validate: build
	cargo run -- validate-specs --from specs

# Install specs to default location + print shell hook instructions
install: build specs
	cargo run -- install-specs --from specs
	@echo ""
	@echo "Specs installed. Add to your .zshrc:"
	@echo '  eval "$$(tabra init zsh)"'

# Clean build artifacts
clean:
	cargo clean
	rm -rf .withfig-autocomplete
