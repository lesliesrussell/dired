# dired-ie4: build, check, and install with Just.
set positional-arguments
# dired-ct2: install into a user-owned directory by default.
PREFIX := env("PREFIX", env("HOME") / ".local")
prefix := replace_regex(PREFIX, "^~", env("HOME"))
BINDIR := env("BINDIR", prefix / "bin")
CARGO := env("CARGO", "cargo")
DATADIR := env("DATADIR", prefix / "share/dired")
MANDIR := env("MANDIR", prefix / "share/man")
DESTDIR := env("DESTDIR", "")

all: release

build:
    {{ CARGO }} build

release:
    {{ CARGO }} build --release --locked
    just examples

examples:
    install -d target/release/examples
    install -m 0644 examples/*.toml target/release/examples/

test:
    {{ CARGO }} test --workspace --locked

lint:
    {{ CARGO }} clippy --workspace --all-targets --locked -- -D warnings

# Uses mandoc when available (macOS, BSD); otherwise groff's warnings.
man-lint:
    @if command -v mandoc >/dev/null 2>&1; then mandoc -Tlint -W warning man/*.1; \
    elif command -v groff >/dev/null 2>&1; then groff -mdoc -z -ww man/*.1; \
    else echo "man-lint: neither mandoc nor groff found; skipping"; fi

check: test lint man-lint

run *args:
    {{ CARGO }} run --release -- "$@"

install: release
    install -d "{{ DESTDIR }}{{ BINDIR }}"
    install -m 0755 target/release/dired "{{ DESTDIR }}{{ BINDIR }}/dired"
    install -d "{{ DESTDIR }}{{ DATADIR }}/examples"
    install -m 0644 examples/*.toml "{{ DESTDIR }}{{ DATADIR }}/examples/"
    install -d "{{ DESTDIR }}{{ MANDIR }}/man1"
    install -m 0644 man/*.1 "{{ DESTDIR }}{{ MANDIR }}/man1/"

uninstall:
    rm -f "{{ DESTDIR }}{{ BINDIR }}/dired"
    rm -rf "{{ DESTDIR }}{{ DATADIR }}"
    for page in man/*.1; do rm -f "{{ DESTDIR }}{{ MANDIR }}/man1/$(basename "$page")"; done

clean:
    {{ CARGO }} clean
