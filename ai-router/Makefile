# Build and install the airouter binary.
#   make
#   sudo make install
#   make install PREFIX=$HOME/.local
#   make redeploy
#   make install-service
# DESTDIR is honored for packaging.
# install-service enables a per-user systemd unit (starts at boot when linger is on).
# redeploy stops that unit, rebuilds, installs, and starts it again.
#
# sudo resets PATH and HOME, which hides rustup's cargo (~/.cargo/bin).
# `sudo make install` compiles as the invoking user, then installs as root.

PREFIX  ?= /usr/local
BINDIR  ?= $(PREFIX)/bin
DESTDIR ?=

TARGET_DIR ?= target
BIN := $(TARGET_DIR)/release/airouter

# SUDO_USER is set by sudo. Ignore it when it is root or empty.
BUILD_HOME :=
ifneq ($(filter-out root,$(SUDO_USER)),)
BUILD_HOME := $(shell getent passwd '$(SUDO_USER)' 2>/dev/null | cut -d: -f6)
endif
ifeq ($(strip $(BUILD_HOME)),)
BUILD_HOME := $(HOME)
endif

ifeq ($(origin CARGO),default)
CARGO := $(shell if command -v cargo >/dev/null 2>&1; then command -v cargo; \
	elif [ -x '$(BUILD_HOME)/.cargo/bin/cargo' ]; then echo '$(BUILD_HOME)/.cargo/bin/cargo'; \
	else echo cargo; fi)
endif

.PHONY: all build clean install uninstall redeploy stop start install-service help

USER_UNIT_DIR := $(BUILD_HOME)/.config/systemd/user
USER_UNIT := $(USER_UNIT_DIR)/airouter.service

all: build

help:
	@echo "make            release build ($(BIN))"
	@echo "make install    install to $(DESTDIR)$(BINDIR)/airouter"
	@echo "make uninstall  remove $(DESTDIR)$(BINDIR)/airouter"
	@echo "make install-service  enable the user systemd unit (boot via linger)"
	@echo "make redeploy  stop the service, clean release build, install, start"
	@echo "PREFIX=$(PREFIX)  BINDIR=$(BINDIR)  DESTDIR=$(DESTDIR)"

# $(1) is the cargo arguments, for example "build --release".
define CARGO_AS_BUILDER
set -eu; \
home='$(BUILD_HOME)'; \
user='$(SUDO_USER)'; \
cargo='$(CARGO)'; \
if [ ! -x "$$cargo" ] && [ -x "$$home/.cargo/bin/cargo" ]; then \
	cargo="$$home/.cargo/bin/cargo"; \
fi; \
path="$$home/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"; \
if [ "$$(id -u)" -eq 0 ] && [ -n "$$user" ] && [ "$$user" != root ]; then \
	echo "cargo as $$user"; \
	if command -v runuser >/dev/null 2>&1; then \
		exec runuser -u "$$user" -- env \
			HOME="$$home" \
			CARGO_HOME="$$home/.cargo" \
			RUSTUP_HOME="$$home/.rustup" \
			PATH="$$path" \
			"$$cargo" $(1); \
	fi; \
	exec su -s /bin/sh "$$user" -c "exec env HOME='$$home' CARGO_HOME='$$home/.cargo' RUSTUP_HOME='$$home/.rustup' PATH='$$path' '$$cargo' $(1)"; \
fi; \
exec env \
	HOME="$$home" \
	CARGO_HOME="$${CARGO_HOME:-$$home/.cargo}" \
	RUSTUP_HOME="$${RUSTUP_HOME:-$$home/.rustup}" \
	PATH="$$home/.cargo/bin:$$PATH" \
	"$$cargo" $(1)
endef

build:
	@$(call CARGO_AS_BUILDER,build --release)

clean:
	@$(call CARGO_AS_BUILDER,clean --release)

# Stop the user service (so systemd does not restart it mid-build), replace the
# installed binary with a clean release build, and start the service again.
# `airouter tui` attaches to that server. sudo is used for install when BINDIR
# is not writable.
redeploy: install-service stop
	$(MAKE) clean
	@if [ "$$(id -u)" -eq 0 ] || [ -w '$(DESTDIR)$(BINDIR)' ]; then \
		$(MAKE) install; \
	else \
		echo "installing to $(DESTDIR)$(BINDIR) with sudo"; \
		sudo $(MAKE) install; \
	fi
	$(MAKE) start

# Runs systemctl --user as the build user. $(1) is the systemctl arguments.
# One line so it can sit inside a larger recipe shell.
define SYSTEMCTL_USER
home='$(BUILD_HOME)'; user='$(SUDO_USER)'; if [ "$$(id -u)" -eq 0 ] && [ -n "$$user" ] && [ "$$user" != root ]; then uid="$$(id -u "$$user")"; runuser -u "$$user" -- env HOME="$$home" XDG_RUNTIME_DIR="/run/user/$$uid" systemctl --user $(1); else systemctl --user $(1); fi
endef

install-service:
	@set -eu; \
	home='$(BUILD_HOME)'; \
	user='$(SUDO_USER)'; \
	unit_dir='$(USER_UNIT_DIR)'; \
	bin='$(BINDIR)/airouter'; \
	install_unit() { \
		mkdir -p "$$unit_dir"; \
		sed "s|@BIN@|$$bin|" deploy/airouter.user.service > "$$unit_dir/airouter.service"; \
	}; \
	if [ "$$(id -u)" -eq 0 ] && [ -n "$$user" ] && [ "$$user" != root ]; then \
		runuser -u "$$user" -- mkdir -p "$$unit_dir"; \
		sed "s|@BIN@|$$bin|" deploy/airouter.user.service | runuser -u "$$user" -- tee "$$unit_dir/airouter.service" >/dev/null; \
	else \
		install_unit; \
	fi; \
	$(call SYSTEMCTL_USER,daemon-reload); \
	$(call SYSTEMCTL_USER,enable airouter.service); \
	linger_user="$${user:-$$(id -un)}"; \
	if [ "$$linger_user" = root ] || [ -z "$$linger_user" ]; then linger_user="$$(id -un)"; fi; \
	if [ "$$(loginctl show-user "$$linger_user" -p Linger --value 2>/dev/null || true)" != yes ]; then \
		echo "enabling linger for $$linger_user so the service starts at boot"; \
		sudo loginctl enable-linger "$$linger_user"; \
	fi; \
	echo "user service enabled ($$bin)"

stop:
	@set -eu; \
	stopped_unit=0; \
	if [ -f '$(USER_UNIT)' ]; then \
		echo "stopping airouter.service"; \
		$(call SYSTEMCTL_USER,stop airouter.service); \
		stopped_unit=1; \
	fi; \
	if ! pgrep -x airouter >/dev/null 2>&1; then \
		if [ "$$stopped_unit" -eq 0 ]; then echo "airouter is not running"; fi; \
		exit 0; \
	fi; \
	echo "stopping leftover airouter processes"; \
	pkill -TERM -x airouter || true; \
	i=0; \
	while pgrep -x airouter >/dev/null 2>&1; do \
		i=$$((i + 1)); \
		if [ "$$i" -ge 25 ]; then \
			echo "airouter did not exit; killing"; \
			pkill -KILL -x airouter || true; \
			sleep 0.2; \
			break; \
		fi; \
		sleep 0.2; \
	done; \
	if pgrep -x airouter >/dev/null 2>&1; then \
		echo "airouter is still running" >&2; \
		exit 1; \
	fi

start:
	@set -eu; \
	home='$(BUILD_HOME)'; \
	user='$(SUDO_USER)'; \
	bin='$(DESTDIR)$(BINDIR)/airouter'; \
	state="$$home/.local/state/airouter"; \
	log="$$state/serve.log"; \
	pidfile="$$state/serve.pid"; \
	if [ ! -x "$$bin" ]; then \
		echo "missing $$bin (run make install)" >&2; \
		exit 1; \
	fi; \
	if [ -f '$(USER_UNIT)' ]; then \
		echo "starting airouter.service"; \
		$(call SYSTEMCTL_USER,start airouter.service); \
		pid="$$($(call SYSTEMCTL_USER,show -p MainPID --value airouter.service))"; \
	else \
		launch() { \
			mkdir -p "$$state"; \
			nohup "$$bin" serve >>"$$log" 2>&1 </dev/null & \
			echo $$! > "$$pidfile"; \
		}; \
		if [ "$$(id -u)" -eq 0 ] && [ -n "$$user" ] && [ "$$user" != root ]; then \
			runuser -u "$$user" -- mkdir -p "$$state"; \
			runuser -u "$$user" -- bash -c 'nohup "$$1" serve >>"$$2" 2>&1 </dev/null & echo $$! > "$$3"' _ "$$bin" "$$log" "$$pidfile"; \
		else \
			launch; \
		fi; \
		pid="$$(cat "$$pidfile")"; \
	fi; \
	health=http://127.0.0.1:8787/health; \
	cfg="$$home/.config/airouter/config.json"; \
	if [ -f "$$cfg" ]; then \
		listen="$$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["server"]["listen"])' "$$cfg")"; \
		case "$$listen" in \
			http://*|https://*) health="$$listen/health" ;; \
			0.0.0.0:*|\[::\]:*) health="http://127.0.0.1:$${listen##*:}/health" ;; \
			*) health="http://$$listen/health" ;; \
		esac; \
	fi; \
	ok=0; \
	i=0; \
	while [ "$$i" -lt 50 ]; do \
		if curl -sf "$$health" >/dev/null 2>&1; then \
			ok=1; \
			break; \
		fi; \
		if [ -z "$$pid" ] || [ "$$pid" = 0 ] || ! kill -0 "$$pid" 2>/dev/null; then \
			break; \
		fi; \
		i=$$((i + 1)); \
		sleep 0.1; \
	done; \
	if [ "$$ok" -ne 1 ]; then \
		echo "airouter $$pid did not become healthy at $$health" >&2; \
		if [ -f '$(USER_UNIT)' ]; then \
			echo "journal: journalctl --user -u airouter.service -n 40 --no-pager" >&2; \
		else \
			echo "log: $$log" >&2; \
		fi; \
		exit 1; \
	fi; \
	exe="$$(readlink -f "/proc/$$pid/exe")"; \
	want="$$(readlink -f "$$bin")"; \
	if [ "$$exe" != "$$want" ]; then \
		echo "running $$exe, installed binary is $$want" >&2; \
		exit 1; \
	fi; \
	echo "airouter $$pid $$exe"

install: build
	install -d $(DESTDIR)$(BINDIR)
	install -m 0755 $(BIN) $(DESTDIR)$(BINDIR)/airouter

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/airouter
