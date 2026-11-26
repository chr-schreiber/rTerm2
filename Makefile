# rterm2 – Bauen und Installieren
#
#   make                 Release-Build (target/release/rterm2)
#   make install         nach ~/.local installieren (Programm + Startmenü-Eintrag)
#   make config          Config-Vorlage nach ~/.config/rterm2/config.toml (überschreibt nichts)
#   make uninstall       wieder entfernen
#
# Systemweit:            sudo make install PREFIX=/usr/local
# Für Pakete:            make install DESTDIR=/tmp/pkg PREFIX=/usr

PREFIX  ?= $(HOME)/.local
BINDIR  ?= $(PREFIX)/bin
APPDIR  ?= $(PREFIX)/share/applications
DOCDIR  ?= $(PREFIX)/share/doc/rterm2
DESTDIR ?=

CARGO   ?= cargo
BIN     := target/release/rterm2
CONFIG_DIR := $(or $(XDG_CONFIG_HOME),$(HOME)/.config)/rterm2

.PHONY: all build debug run test check fmt clippy install uninstall config clean help

all: build

## build      Release-Build
build:
	$(CARGO) build --release

## debug      Debug-Build
debug:
	$(CARGO) build

## run        Release-Build starten, mit Log-Ausgabe
run:
	RUST_LOG=rterm2=info $(CARGO) run --release

## test       Unit-Tests
test:
	$(CARGO) test

## check      Kompilieren ohne Binary (schnell)
check:
	$(CARGO) check

## fmt        Code formatieren
fmt:
	$(CARGO) fmt

## clippy     Lint
clippy:
	$(CARGO) clippy --release -- -D warnings

## install    Programm, Startmenü-Eintrag und Doku installieren
install: build
	install -Dm755 $(BIN) "$(DESTDIR)$(BINDIR)/rterm2"
	install -Dm644 rterm2.desktop "$(DESTDIR)$(APPDIR)/rterm2.desktop"
	install -Dm644 README.md README.de.md config.example.toml -t "$(DESTDIR)$(DOCDIR)"
	@if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then \
		update-desktop-database "$(APPDIR)" >/dev/null 2>&1 || true; \
	fi
	@echo "rterm2 installiert nach $(DESTDIR)$(BINDIR)/rterm2"
	@case ":$$PATH:" in *":$(BINDIR):"*) ;; *) \
		[ -n "$(DESTDIR)" ] || echo "Hinweis: $(BINDIR) steht nicht in PATH – das Startmenü findet rterm2 dann nicht." ;; \
	esac

## uninstall  Installierte Dateien entfernen (die eigene Config bleibt)
uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/rterm2"
	rm -f "$(DESTDIR)$(APPDIR)/rterm2.desktop"
	rm -rf "$(DESTDIR)$(DOCDIR)"

## config     config.example.toml als eigene Config anlegen (nur wenn noch keine da ist)
config:
	@if [ -e "$(CONFIG_DIR)/config.toml" ]; then \
		echo "$(CONFIG_DIR)/config.toml gibt es schon – nicht überschrieben."; \
		echo "Vorlage zum Vergleichen: config.example.toml"; \
	else \
		install -Dm644 config.example.toml "$(CONFIG_DIR)/config.toml"; \
		echo "Config angelegt: $(CONFIG_DIR)/config.toml"; \
	fi

## clean      Build-Verzeichnis löschen
clean:
	$(CARGO) clean

## help       diese Übersicht
help:
	@grep -E '^## ' $(MAKEFILE_LIST) | sed 's/^## /  make /'
