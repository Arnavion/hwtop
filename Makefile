.PHONY: build clean install test

default: build

clean:
	rm -rf Cargo.lock target/

build:
	cargo build --release -p hwtop -p sensord

install: build
	cp -f target/release/hwtop ~/.local/bin/

	sudo mkdir -p /usr/local/bin/
	sudo cp -f target/release/sensord /usr/local/bin/
	if [ -d '/etc/systemd/system/' ]; then \
		sudo cp -f sensord/data/systemd/sensord.service /etc/systemd/system/; \
		sudo systemctl daemon-reload; \
	elif [ -d '/etc/init.d/' ]; then \
		sudo cp -f sensord/data/openrc/sensord /etc/init.d/; \
	fi
	sudo cp -f sensord/data/systemd/sensord.conf /etc/dbus-1/system.d/

test:
	cargo test --all
	cargo clippy --all
