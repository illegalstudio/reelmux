CARGO ?= cargo
VERSION ?= $(shell sed -n -E '0,/^version = "/s/^version = "([^"]+)".*/\1/p' Cargo.toml)
ARCH ?= $(shell uname -m | sed -e 's/x86_64/amd64/' -e 's/aarch64/arm64/')

.PHONY: all build test lint check dist appimage packages release clean

all: build

build:
	$(CARGO) build --locked --release

test:
	$(CARGO) test --locked

lint:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --locked --all-targets -- -D warnings

check: lint test

dist: build
	scripts/package.sh target/release/reelmux $(VERSION) $(ARCH)

appimage: build
	scripts/package-appimage.sh target/release/reelmux $(VERSION) $(ARCH)

packages: dist appimage

release:
	@scripts/release.sh

clean:
	$(CARGO) clean
	rm -rf dist
