This repository contains:

- `hwtop` - TUI monitor for CPU usage, thermal and fan sensors, and network usage.

- `sensord` - D-Bus service that broadcasts CPU usage, thermal and fan sensors, and network usage information as a periodic signal. Used by `hwtop`.

See their respective READMEs for more details.


# Installation

1. Build and install the `sensord` and `hwtop` binaries (to `/usr/local/bin` and `~/bin` respectively) and the `sensord` systemd service and D-Bus config files:

   ```sh
   make install
   ```

   Make sure to run it as your regular user, ie not as root and without `sudo`.

1. Create the configuration for both services as described in their respective READMEs.

1. Start the `sensord` service.

   systemd:

   ```sh
   sudo systemctl start sensord

   # sudo systemctl enable sensord   # To start it automatically on boot
   ```

   openrc:

   ```sh
   sudo rc-service sensord start

   # sudo rc-update add sensord default   # To start it automatically on boot
   ```

1. Start `hwtop` in a terminal.

   ```sh
   hwtop
   ```


# License

```
hwtop

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
