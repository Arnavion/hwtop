`sensord` is a D-Bus service that broadcasts CPU usage, thermal and fan sensors, and network usage information.


# Build and install

Running `make install` from the repository root will already install the binary, systemd service, and D-Bus config.

To install manually instead, run:

```sh
cargo build --release

sudo cp -f ./target/release/sensord /usr/local/bin/

sudo cp -f ./data/sensord.service /etc/systemd/system/
sudo cp -f ./data/sensord.conf /etc/dbus-1/system.d/
sudo systemctl daemon-reload
```


# Run

1. Create a config file at `/etc/sensord/config.yaml` See the examples under `config-examples/` for reference.

1. `sudo systemctl start sensord`


# License

```
sensord

https://github.com/Arnavion/hwtop

Copyright 2019 Arnav Singh

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

   http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
