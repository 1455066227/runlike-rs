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

Download the archive for your platform from
[GitHub Releases](../../releases):

| Platform              | Asset                        |
| --------------------- | ---------------------------- |
| Windows x86_64        | `runlike-windows-amd64.zip`  |
| Linux x86_64          | `runlike-linux-amd64.tar.gz` |
| Linux aarch64         | `runlike-linux-aarch64.tar.gz` |
| macOS (Apple Silicon) | `runlike-macos-arm64.tar.gz` |

Unpack it and put the `runlike` binary (or `runlike.exe` on Windows) somewhere
in your `PATH`.

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
$ git tag v1.6.3
$ git push origin v1.6.3
```

The workflow runs formatting, Clippy and the test suite, then builds and
publishes these assets to the GitHub release:

- `runlike-windows-amd64.zip`
- `runlike-linux-amd64.tar.gz`
- `runlike-linux-aarch64.tar.gz`
- `runlike-macos-arm64.tar.gz`

## License

BSD 3-Clause. See [LICENSE](LICENSE). The original project is Copyright (c)
2015, Assaf Lavie.