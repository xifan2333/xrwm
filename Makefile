PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
MANDIR ?= $(PREFIX)/share/man/man1
DATADIR ?= $(PREFIX)/share/wayland-sessions
DOCDIR ?= $(PREFIX)/share/doc/xrwm
TARGET ?= $(shell [ -f target/release/xrwm ] && echo target/release/xrwm || echo xrwm)

all:
	cargo build --release

install:
	install -Dm755 $(TARGET) $(DESTDIR)$(BINDIR)/xrwm
	install -Dm644 doc/xrwm.1 $(DESTDIR)$(MANDIR)/xrwm.1
	install -Dm644 examples/xrwm.desktop $(DESTDIR)$(DATADIR)/xrwm.desktop
	install -Dm644 examples/init $(DESTDIR)$(DOCDIR)/examples/init

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/xrwm
	rm -f $(DESTDIR)$(MANDIR)/xrwm.1
	rm -f $(DESTDIR)$(DATADIR)/xrwm.desktop
	rm -rf $(DESTDIR)$(DOCDIR)

.PHONY: all install uninstall
