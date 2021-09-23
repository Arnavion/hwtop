.PHONY: build clean install test

default: target/release/hwtop target/release/sensord

clean:
	rm -rf Cargo.lock target/

target/release/hwtop target/release/sensord &:
	cargo build --release -p hwtop -p sensord

install: target/release/hwtop target/release/sensord
	cp -f target/release/hwtop target/release/sensord /usr/local/bin/
	if [ -d '/etc/systemd/system/' ]; then \
		cp -f sensord/data/systemd/sensord.service /etc/systemd/system/; \
		systemctl daemon-reload; \
	elif [ -d '/etc/init.d/' ]; then \
		cp -f sensord/data/openrc/sensord /etc/init.d/; \
	fi
	cp -f sensord/data/systemd/sensord.conf /etc/dbus-1/system.d/

test:
	cargo test --all
	cargo clippy --all
