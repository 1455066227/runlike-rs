# runlike (Rust)

A Rust port of [runlike](https://github.com/lavie/runlike): give it a running
Docker container and it prints the `docker run` command that would create an
equivalent container, including ports, links, volumes, environment variables,
labels, log settings, and so on.

The original Python implementation is kept in `runlike-python/` for reference and
is excluded from this repository via `.gitignore`.

## Usage

```console
$ runlike <container-name>
docker run --name=my-container --hostname=... -p 8080:80 ... my-image
```

Run the printed command directly:

```console
$ $(runlike my-container)
```

Pretty-print one option per line:

```console
$ runlike -p my-container
docker run \
        --name=my-container \
        ...
```

Read `docker inspect` output from stdin instead of calling Docker:

```console
$ docker inspect my-container | runlike --stdin
```

### Options

| Option              | Description                                                   |
| ------------------- | ------------------------------------------------------------- |
| `-p`, `--pretty`    | Break the command into one option per line                    |
| `-s`, `--stdin`     | Read container facts from `docker inspect` JSON on stdin      |
| `-l`, `--no-labels` | Do not include labels in the output                           |
| `--no-name`         | Do not include the container name in the output               |
| `--use-volume-id`   | Use the automatically assigned volume name instead of its path for anonymous volumes |
| `-h`, `--help`      | Show help                                                     |
| `-V`, `--version`   | Show version                                                  |

`runlike` needs the `docker` CLI available in `PATH` unless `--stdin` is used.

## Install

Download the single binary for your platform from
[GitHub Releases](../../releases):

| Platform              | Asset                    |
| --------------------- | ------------------------ |
| Windows x86_64        | `runlike-windows-amd64.exe` |
| Linux x86_64          | `runlike-linux-amd64`    |
| Linux aarch64         | `runlike-linux-aarch64`  |
| macOS (Apple Silicon) | `runlike-macos-arm64`    |

On Linux/macOS make it executable and put it somewhere in your `PATH`:

```console
$ chmod +x runlike-linux-amd64
$ sudo mv runlike-linux-amd64 /usr/local/bin/runlike
```

On Windows just rename `runlike-windows-amd64.exe` to `runlike.exe` and put it
in your `PATH`.

The Linux binaries are built against musl and statically linked, so they do not
depend on the host's glibc version or on any bundled shared libraries — copy the
single file to any x86_64 or aarch64 Linux distribution and run it.

## Build from source

```console
$ cargo build --release
$ ./target/release/runlike --help
```

Run the unit tests:

```console
$ cargo test
```

## Release process

Releases are built by GitHub Actions (`.github/workflows/release.yml`) whenever a
tag starting with `v` is pushed:

```console
$ git tag v1.6.5
$ git push origin v1.6.5
```

The workflow runs formatting, Clippy and the test suite, then builds and
publishes these raw executables to the GitHub release:

- `runlike-windows-amd64.exe`
- `runlike-linux-amd64` (statically linked with musl)
- `runlike-linux-aarch64` (statically linked with musl)
- `runlike-macos-arm64`

## License

BSD 3-Clause. See [LICENSE](LICENSE). The original project is Copyright (c)
2015, Assaf Lavie.