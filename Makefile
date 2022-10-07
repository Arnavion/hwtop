.PHONY: clean default install test

default: target/release/hwtop target/release/sensord

clean:
	rm -rf Cargo.lock target/

target/release/hwtop target/release/sensord &:
	cargo build --release -p hwtop -p sensord

install: target/release/hwtop target/release/sensord
	cp -f target/release/hwtop target/release/sensord /usr/local/bin/
	if [ -d '/etc/systemd/system/' ]; then \
		cp -f sensord/data/systemd/sensord.service /etc/systemd/system/; \
		mkdir -p /etc/sysusers.d/; \
		cp -f sensord/data/systemd/sensord.sysusers /etc/sysusers.d/system-user-sensord.conf; \
		systemctl daemon-reload; \
	elif [ -d '/etc/init.d/' ]; then \
		cp -f sensord/data/openrc/sensord.init /etc/init.d/sensord; \
		if ! /usr/bin/getent passwd sensord >/dev/null; then \
			/usr/sbin/useradd --system --comment 'dev.arnavion.sensord' --shell /sbin/nologin --no-create-home sensord; \
		fi; \
	fi
	mkdir -p /etc/dbus-1/system.d/
	cp -f sensord/data/sensord.dbus /etc/dbus-1/system.d/sensord.conf

test:
	cargo test --all
	cargo clippy --all --tests --examples
