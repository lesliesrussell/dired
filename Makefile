# dired-7gc
# dired-7yu
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO ?= cargo
DATADIR ?= $(PREFIX)/share/dired
EXAMPLES := $(wildcard examples/*.toml)
BIN := dired

.PHONY: all build release examples test lint check install uninstall clean run

all: release

build:
	$(CARGO) build

release:
	$(CARGO) build --release --locked
	$(MAKE) examples

examples:
	install -d target/release/examples
	install -m 0644 $(EXAMPLES) target/release/examples/

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
	install -d "$(DESTDIR)$(DATADIR)/examples"
	install -m 0644 $(EXAMPLES) "$(DESTDIR)$(DATADIR)/examples/"

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/$(BIN)"
	rm -rf "$(DESTDIR)$(DATADIR)"

clean:
	$(CARGO) clean
