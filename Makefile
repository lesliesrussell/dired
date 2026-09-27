# dired-7gc
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO ?= cargo
BIN := dired

.PHONY: all build release test lint check install uninstall clean run

all: release

build:
	$(CARGO) build

release:
	$(CARGO) build --release --locked

test:
	$(CARGO) test --workspace --locked

lint:
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings

check: test lint

run:
	$(CARGO) run --release -- $(ARGS)

install: release
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 0755 target/release/$(BIN) "$(DESTDIR)$(BINDIR)/$(BIN)"

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/$(BIN)"

clean:
	$(CARGO) clean
