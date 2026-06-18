.PHONY: all build run test clean apk

all: build

build:
	scripts/build.sh

run:
	cargo run --release --manifest-path relay-rust/Cargo.toml -- run

test:
	cargo test --manifest-path relay-rust/Cargo.toml

apk:
	scripts/build-apk.sh

release:
	scripts/release.sh

clean:
	cargo clean --manifest-path relay-rust/Cargo.toml
	./gradlew clean 2>/dev/null || true
	rm -rf dist
