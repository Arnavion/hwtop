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
	sudo cp -f sensord/data/sensord.service /etc/systemd/system/
	sudo cp -f sensord/data/sensord.conf /etc/dbus-1/system.d/
	sudo systemctl daemon-reload

test:
	cargo test --all
	cargo clippy --all
