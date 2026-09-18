PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
MANDIR ?= $(PREFIX)/share/man/man1
DATADIR ?= $(PREFIX)/share/wayland-sessions
DOCDIR ?= $(PREFIX)/share/doc/xrwm
TARGET ?= $(shell [ -f xrwm ] && echo xrwm || echo target/release/xrwm)

all: $(TARGET)

target/release/xrwm: FORCE
	cargo build --release

doc:
	@date=$$(git log -1 --format=%cd --date=format:'%B %Y' doc/xrwm.1.md 2>/dev/null || date +"%B %Y"); \
	version=$$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1); \
	pandoc -s -t man doc/xrwm.1.md -M date="$$date" -M footer="xrwm $$version" -o doc/xrwm.1

install: $(TARGET)
	install -Dm755 $(TARGET) $(DESTDIR)$(BINDIR)/xrwm
	install -Dm644 doc/xrwm.1 $(DESTDIR)$(MANDIR)/xrwm.1
	install -Dm644 examples/xrwm.desktop $(DESTDIR)$(DATADIR)/xrwm.desktop
	install -Dm644 examples/init $(DESTDIR)$(DOCDIR)/examples/init

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/xrwm
	rm -f $(DESTDIR)$(MANDIR)/xrwm.1
	rm -f $(DESTDIR)$(DATADIR)/xrwm.desktop
	rm -rf $(DESTDIR)$(DOCDIR)

FORCE:

.PHONY: all doc install uninstall FORCE
