NAME   = perl-critic-sarif
TARGET = x86_64-unknown-linux-musl

.PHONY: release
release:
	cross build --target=$(TARGET) --release

.PHONY: debug
debug:
	cross build --target=$(TARGET)

.PHONY: test
test:
	cargo clippy --all-features
	cargo test
