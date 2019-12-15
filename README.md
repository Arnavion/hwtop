TUI monitor for CPU usage, thermal and fan sensors, and network usage.


# Build

```sh
cargo build --release
```


# Install

```sh
cp -f ./target/release/hwtop ~/bin/
```


# Run

1. Create a config file at `~/.config/hwtop/hwtop.yaml` See the examples under `config-examples/` for reference.

1. `hwtop`


# Example output

(The actual output uses colors that are not visible here.)

1. Output for the device corresponding to `raspberry-pi.yaml`

    ```
      0:   0.0% 600.0 MHz    1:   1.0% 600.0 MHz    2:   0.0% 600.0 MHz    3:   3.0% 600.0 MHz
    Avg:   1.0%

    CPU:  47.2°C

    eth0:   2.1 KB/s down     1.5 KB/s up    Press i to show sensor names, q to exit
    ```

1. Output for the device corresponding to `t61.yaml`

    ```
      0:   0.0% 797.9 MHz    1:   7.0% 797.9 MHz
    Avg:   3.5%

     CPU:  48.0°C   45.0°C   53.0°C
     GPU:  59.0°C   50.0°C
    Mobo:  42.0°C   38.0°C   31.0°C   40.0°C   48.0°C   46.0°C   14% (3236 RPM)
    Mobo:   N/A     28.0°C    N/A      N/A

    enp0s25:    140 B/s down     1.0 KB/s up    Press i to show sensor names, q to exit
    ```

1. Output for the device corresponding to `threadripper2.yaml`

    ```
      0:   2.0% 2.086 GHz    6:   0.0% 2.313 GHz   12:   0.0% 2.053 GHz   18:  18.0% 3.204 GHz
      1:   1.0% 2.111 GHz    7:   0.0% 2.057 GHz   13:   0.0% 2.143 GHz   19:   3.0% 2.165 GHz
      2:   1.0% 2.099 GHz    8:   0.0% 2.054 GHz   14:   0.0% 2.113 GHz   20:   0.0% 1.970 GHz
      3:   1.0% 1.957 GHz    9:   1.0% 2.805 GHz   15:   0.0% 2.060 GHz   21:   3.0% 3.477 GHz
      4:   0.0% 1.909 GHz   10:   0.0% 1.938 GHz   16:   0.0% 1.918 GHz   22:   0.0% 1.972 GHz
      5:   0.0% 2.004 GHz   11:   0.0% 2.184 GHz   17:   0.0% 2.094 GHz   23:   2.0% 2.155 GHz
    Avg:   1.3%

     CPU:  30.0°C   30.0°C   30.0°C            69% (1002 RPM)
     GPU:  28.0°C   29.0°C   29.0°C            25% ( 835 RPM)
    Mobo:  30.0°C   40.0°C   33.0°C   36.0°C   65% ( 624 RPM)   65% ( 779 RPM)   65% ( 704 RPM)

    enp4s0:   4.1 KB/s down      476 B/s up    Press i to show sensor names, q to exit
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
