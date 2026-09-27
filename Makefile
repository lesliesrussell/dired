# dired-7gc
# dired-7yu
# dired-ji3
PREFIX ?= /usr/local
# dired-9m8
# zsh passes `PREFIX=~/.local` through unexpanded; expand a leading ~ here.
override PREFIX := $(patsubst ~/%,$(HOME)/%,$(patsubst ~,$(HOME),$(PREFIX)))
BINDIR ?= $(PREFIX)/bin
CARGO ?= cargo
DATADIR ?= $(PREFIX)/share/dired
EXAMPLES := $(wildcard examples/*.toml)
MANDIR ?= $(PREFIX)/share/man
MANPAGES := $(wildcard man/*.1)
BIN := dired

.PHONY: all build release examples test lint man-lint check install uninstall clean run

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

# Uses mandoc when available (macOS, BSD); otherwise groff's warnings.
man-lint:
	@if command -v mandoc >/dev/null 2>&1; then mandoc -Tlint -W warning $(MANPAGES); \
	elif command -v groff >/dev/null 2>&1; then groff -mdoc -z -ww $(MANPAGES); \
	else echo "man-lint: neither mandoc nor groff found; skipping"; fi

check: test lint man-lint

run:
	$(CARGO) run --release -- $(ARGS)

install: release
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 0755 target/release/$(BIN) "$(DESTDIR)$(BINDIR)/$(BIN)"
	install -d "$(DESTDIR)$(DATADIR)/examples"
	install -m 0644 $(EXAMPLES) "$(DESTDIR)$(DATADIR)/examples/"
	install -d "$(DESTDIR)$(MANDIR)/man1"
	install -m 0644 $(MANPAGES) "$(DESTDIR)$(MANDIR)/man1/"

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/$(BIN)"
	rm -rf "$(DESTDIR)$(DATADIR)"
	rm -f $(addprefix "$(DESTDIR)$(MANDIR)/man1/,$(addsuffix ",$(notdir $(MANPAGES))))

clean:
	$(CARGO) clean
