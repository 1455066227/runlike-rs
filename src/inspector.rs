//! Inspect a Docker container and rebuild the equivalent `docker run` command.
//!
//! This is a Rust port of the Python `runlike` package. The option order and
//! formatting deliberately mirror the original implementation so that the
//! output stays familiar.

use std::process::Command;

use serde_json::{Map, Value};

use crate::shell::{quote, quote_command_arg};

/// Errors are plain strings because they are shown to the user verbatim.
pub type Result<T> = std::result::Result<T, String>;

pub struct Inspector {
    pub container: Option<String>,
    pub no_name: bool,
    pub no_labels: bool,
    pub use_volume_id: bool,
    pub pretty: bool,
    container_facts: Option<Value>,
    image_facts: Option<Value>,
    options: Vec<String>,
}

impl Inspector {
    pub fn new(
        container: Option<String>,
        no_name: bool,
        use_volume_id: bool,
        pretty: bool,
        no_labels: bool,
    ) -> Self {
        Self {
            container,
            no_name,
            no_labels,
            use_volume_id,
            pretty,
            container_facts: None,
            image_facts: None,
            options: Vec::new(),
        }
    }

    /// Run `docker container inspect` and `docker image inspect`.
    pub fn inspect(&mut self) -> Result<()> {
        let container = self
            .container
            .clone()
            .ok_or_else(|| "no container specified".to_string())?;

        let output = match run_docker(&["container", "inspect", &container]) {
            Ok(output) => output,
            Err(message) if message.contains("No such container") => {
                return Err(format!("No such container {container}"));
            }
            Err(message) => return Err(message),
        };

        self.container_facts = Some(parse_inspect_output(&output, "container")?);

        let image_hash = self
            .c_str("Image")
            .ok_or_else(|| "docker inspect did not return an image id".to_string())?;

        let output = run_docker(&["image", "inspect", &image_hash])?;

        self.image_facts = Some(parse_inspect_output(&output, "image")?);
        Ok(())
    }

    /// Load container facts from raw `docker inspect` JSON (the `--stdin` mode).
    pub fn set_container_facts(&mut self, raw_json: &str) -> Result<()> {
        let facts: Value = serde_json::from_str(raw_json)
            .map_err(|e| format!("could not parse docker inspect JSON: {e}"))?;
        self.container_facts = Some(validate_inspect_facts(facts, "container")?);
        Ok(())
    }

    /// Build the `docker run ...` command line.
    pub fn format_cli(&mut self) -> Result<String> {
        self.options.clear();

        let name = self
            .c_str("Name")
            .map(|name| name.rsplit('/').next().unwrap_or_default().to_string())
            .unwrap_or_default();
        if !self.no_name {
            self.options.push(format!("--name={name}"));
        }

        self.parse_hostname();
        self.parse_user();
        self.parse_macaddress();
        self.parse_ipv6();
        self.parse_pid();
        self.parse_cpuset();
        self.parse_entrypoint();

        self.parse_volumes();

        self.multi_option("Config.Env", "env");
        self.multi_option("HostConfig.VolumesFrom", "volumes-from");
        self.multi_option("HostConfig.CapAdd", "cap-add");
        self.multi_option("HostConfig.CapDrop", "cap-drop");
        self.multi_option("HostConfig.Dns", "dns");

        if let Some(network_mode) = self.c_str("HostConfig.NetworkMode") {
            if network_mode != "default" {
                self.options.push(format!("--network={network_mode}"));
            }
        }

        if self.c_bool("HostConfig.Privileged").unwrap_or(false) {
            self.options.push("--privileged".to_string());
        }

        self.parse_workdir();
        self.parse_ports();
        self.parse_links();
        self.parse_restart();
        self.parse_devices();
        if !self.no_labels {
            self.parse_labels();
        }
        self.parse_log();
        self.parse_extra_hosts();
        self.parse_runtime();
        self.parse_shm_size();
        self.parse_memory();
        self.parse_memory_reservation();

        if !self.c_bool("Config.AttachStdout").unwrap_or(false) {
            self.options.push("--detach=true".to_string());
        }

        if self.c_bool("Config.Tty").unwrap_or(false) {
            self.options.push("-t".to_string());
        }

        if self.c_bool("HostConfig.AutoRemove").unwrap_or(false) {
            self.options.push("--rm".to_string());
        }

        let image = self
            .c_str("Config.Image")
            .ok_or_else(|| "docker inspect did not return a container image".to_string())?;

        let mut parameters = self.options.clone();
        parameters.push(image);

        let cmd_parts = self
            .container_fact("Config.Cmd")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if !cmd_parts.is_empty() {
            let quoted: Vec<String> = cmd_parts
                .iter()
                .map(|part| quote_command_arg(&value_to_string(part)))
                .collect();
            parameters.push(quoted.join(" "));
        }

        let joiner = if self.pretty { " \\\n\t" } else { " " };
        Ok(format!("docker run {}", parameters.join(joiner)))
    }

    // ------------------------------------------------------------------
    // Fact access helpers
    // ------------------------------------------------------------------

    fn container_fact(&self, path: &str) -> Option<&Value> {
        let root = self.container_facts.as_ref()?.as_array()?.first()?;
        get_fact(root, path)
    }

    fn image_fact(&self, path: &str) -> Option<&Value> {
        let root = self.image_facts.as_ref()?.as_array()?.first()?;
        get_fact(root, path)
    }

    fn c_str(&self, path: &str) -> Option<String> {
        self.container_fact(path)?.as_str().map(str::to_string)
    }

    fn c_bool(&self, path: &str) -> Option<bool> {
        self.container_fact(path)?.as_bool()
    }

    fn c_u64(&self, path: &str) -> Option<u64> {
        self.container_fact(path)?.as_u64()
    }

    // ------------------------------------------------------------------
    // Option parsers
    // ------------------------------------------------------------------

    fn multi_option(&mut self, path: &str, option: &str) {
        let container_values = self
            .container_fact(path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let image_values = self
            .image_fact(path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for value in container_values {
            // Ignore values that are already part of the image definition.
            if image_values.contains(&value) {
                continue;
            }
            let text = value_to_string(&value);
            self.options.push(format!("--{option}={}", quote(&text)));
        }
    }

    fn parse_hostname(&mut self) {
        if let Some(hostname) = self.c_str("Config.Hostname") {
            self.options.push(format!("--hostname={hostname}"));
        }
    }

    fn parse_user(&mut self) {
        if let Some(user) = self.c_str("Config.User") {
            if !user.is_empty() {
                self.options.push(format!("--user={user}"));
            }
        }
    }

    fn parse_macaddress(&mut self) {
        let mac_address = self
            .c_str("Config.MacAddress")
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.c_str("NetworkSettings.MacAddress")
                    .filter(|value| !value.is_empty())
            });

        if let Some(mac_address) = mac_address {
            self.options.push(format!("--mac-address={mac_address}"));
        }
    }

    fn parse_ipv6(&mut self) {
        let networks = self
            .container_fact("NetworkSettings.Networks")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        for network in networks.values() {
            let address = network
                .get("IPAMConfig")
                .and_then(Value::as_object)
                .and_then(|config| config.get("IPv6Address"))
                .and_then(Value::as_str);

            if let Some(address) = address {
                if !address.is_empty() {
                    self.options.push(format!("--ip6={address}"));
                    break;
                }
            }
        }
    }

    fn parse_ports(&mut self) {
        let mut ports: Map<String, Value> = self
            .container_fact("NetworkSettings.Ports")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        if let Some(bindings) = self
            .container_fact("HostConfig.PortBindings")
            .and_then(Value::as_object)
        {
            for (key, value) in bindings {
                ports.insert(key.clone(), value.clone());
            }
        }

        for (container_port_and_protocol, bindings) in ports {
            let (container_port, protocol) = container_port_and_protocol
                .split_once('/')
                .unwrap_or((container_port_and_protocol.as_str(), "tcp"));
            let protocol_part = if protocol == "tcp" { "" } else { "/udp" };

            match bindings {
                Value::Null => {
                    self.options
                        .push(format!("--expose={container_port}{protocol_part}"));
                }
                Value::Array(hosts) => {
                    for host in hosts {
                        let host_ip = host
                            .get("HostIp")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let host_port = host
                            .get("HostPort")
                            .and_then(Value::as_str)
                            .unwrap_or_default();

                        let host_port_part = if !host_port.is_empty() && host_port != "0" {
                            format!("{host_port}:")
                        } else {
                            String::new()
                        };
                        let hostname_part =
                            if !host_ip.is_empty() && host_ip != "0.0.0.0" && host_ip != "::" {
                                format!("{host_ip}:")
                            } else {
                                String::new()
                            };

                        let option = format!(
                            "-p {hostname_part}{host_port_part}{container_port}{protocol_part}"
                        );

                        if self.options.last() != Some(&option) {
                            self.options.push(option);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_volumes(&mut self) {
        let mounts = self
            .container_fact("Mounts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for mount in mounts {
            let mount_type = mount
                .get("Type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let destination = mount
                .get("Destination")
                .and_then(Value::as_str)
                .unwrap_or_default();

            let mut volume_format = if mount_type == "volume" {
                if self.use_volume_id {
                    let name = mount
                        .get("Name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    format!("{name}:{destination}")
                } else {
                    destination.to_string()
                }
            } else {
                let source = mount
                    .get("Source")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                format!("{source}:{destination}")
            };

            if mount.get("RW").and_then(Value::as_bool) != Some(true) {
                volume_format.push_str(":ro");
            }

            self.options.push(format!("--volume {volume_format}"));
        }
    }

    fn parse_links(&mut self) {
        let links = self
            .container_fact("HostConfig.Links")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut seen: Vec<String> = Vec::new();
        for link in links {
            let Some(link) = link.as_str() else {
                continue;
            };
            let Some((src, dst)) = link.split_once(':') else {
                continue;
            };

            let src = src.rsplit('/').next().unwrap_or(src);
            let dst = dst.rsplit('/').next().unwrap_or(dst);

            let option = if src != dst {
                format!("--link {src}:{dst}")
            } else {
                format!("--link {src}")
            };

            if !seen.contains(&option) {
                seen.push(option);
            }
        }

        self.options.extend(seen);
    }

    fn parse_pid(&mut self) {
        if let Some(mode) = self.c_str("HostConfig.PidMode") {
            if !mode.is_empty() {
                self.options.push(format!("--pid {mode}"));
            }
        }
    }

    fn parse_cpuset(&mut self) {
        if let Some(cpuset_cpu) = self.c_str("HostConfig.CpusetCpus") {
            if !cpuset_cpu.is_empty() {
                self.options.push(format!("--cpuset-cpus={cpuset_cpu}"));
            }
        }

        if let Some(cpuset_mem) = self.c_str("HostConfig.CpusetMems") {
            if !cpuset_mem.is_empty() {
                self.options.push(format!("--cpuset-mems={cpuset_mem}"));
            }
        }
    }

    fn parse_restart(&mut self) {
        let Some(mut restart) = self.c_str("HostConfig.RestartPolicy.Name") else {
            return;
        };

        if restart.is_empty() || restart == "no" {
            return;
        }

        if restart == "on-failure" {
            let max_retries = self
                .c_u64("HostConfig.RestartPolicy.MaximumRetryCount")
                .unwrap_or(0);
            if max_retries > 0 {
                restart = format!("{restart}:{max_retries}");
            }
        }

        self.options.push(format!("--restart={restart}"));
    }

    fn parse_devices(&mut self) {
        let devices = self
            .container_fact("HostConfig.Devices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut seen: Vec<String> = Vec::new();
        for device in devices {
            let host = device
                .get("PathOnHost")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let container = device
                .get("PathInContainer")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let permissions = device
                .get("CgroupPermissions")
                .and_then(Value::as_str)
                .unwrap_or_default();

            let mut spec = format!("{host}:{container}");
            if permissions != "rwm" {
                spec.push(':');
                spec.push_str(permissions);
            }

            let option = format!("--device {spec}");
            if !seen.contains(&option) {
                seen.push(option);
            }
        }

        self.options.extend(seen);
    }

    fn parse_labels(&mut self) {
        let container_labels = self
            .container_fact("Config.Labels")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let image_labels = self
            .image_fact("Config.Labels")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let mut seen: Vec<String> = Vec::new();
        for (key, value) in &container_labels {
            // Ignore labels that are part of the image definition.
            if image_labels.get(key) == Some(value) {
                continue;
            }

            let option = format!("--label='{key}={}'", value_to_string(value));
            if !seen.contains(&option) {
                seen.push(option);
            }
        }

        self.options.extend(seen);
    }

    fn parse_log(&mut self) {
        let log_type = self.c_str("HostConfig.LogConfig.Type");
        let log_opts = self
            .container_fact("HostConfig.LogConfig.Config")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let mut log_options: Vec<String> = Vec::new();

        if let Some(log_type) = log_type {
            if log_type != "json-file" {
                log_options.push(format!("--log-driver={log_type}"));
            }
        }

        for (key, value) in log_opts {
            log_options.push(format!("--log-opt {key}={}", value_to_string(&value)));
        }

        for option in log_options {
            if !self.options.contains(&option) {
                self.options.push(option);
            }
        }
    }

    fn parse_extra_hosts(&mut self) {
        let hosts = self
            .container_fact("HostConfig.ExtraHosts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for host in hosts {
            if let Some(host) = host.as_str() {
                self.options.push(format!("--add-host {host}"));
            }
        }
    }

    fn parse_workdir(&mut self) {
        if let Some(workdir) = self.c_str("Config.WorkingDir") {
            if !workdir.is_empty() {
                self.options.push(format!("--workdir={workdir}"));
            }
        }
    }

    fn parse_runtime(&mut self) {
        if let Some(runtime) = self.c_str("HostConfig.Runtime") {
            if !runtime.is_empty() {
                self.options.push(format!("--runtime={runtime}"));
            }
        }
    }

    fn parse_shm_size(&mut self) {
        if let Some(shm_size) = self.c_u64("HostConfig.ShmSize") {
            if shm_size > 0 {
                self.options.push(format!("--shm-size=\"{shm_size}\""));
            }
        }
    }

    fn parse_memory(&mut self) {
        if let Some(memory) = self.c_u64("HostConfig.Memory") {
            if memory > 0 {
                self.options.push(format!("--memory=\"{memory}\""));
            }
        }
    }

    fn parse_memory_reservation(&mut self) {
        if let Some(memory) = self.c_u64("HostConfig.MemoryReservation") {
            if memory > 0 {
                self.options
                    .push(format!("--memory-reservation=\"{memory}\""));
            }
        }
    }

    fn parse_entrypoint(&mut self) {
        let entrypoints = self
            .container_fact("Config.Entrypoint")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let image_entrypoints = self
            .image_fact("Config.Entrypoint")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if !entrypoints.is_empty() && entrypoints != image_entrypoints {
            let entrypoint = entrypoints.first().map(value_to_string).unwrap_or_default();
            self.options.push(format!("--entrypoint {entrypoint}"));
        }
    }
}

fn get_fact<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn run_docker(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run `docker {}`: {e}", args.join(" ")))?;

    if output.status.success() {
        return Ok(output.stdout);
    }

    let mut message = String::new();
    message.push_str(&String::from_utf8_lossy(&output.stdout));
    message.push_str(&String::from_utf8_lossy(&output.stderr));
    let message = message.trim();

    if message.is_empty() {
        Err(format!(
            "`docker {}` exited with status {}",
            args.join(" "),
            output.status
        ))
    } else {
        Err(message.to_string())
    }
}

fn parse_inspect_output(output: &[u8], kind: &str) -> Result<Value> {
    let facts: Value = serde_json::from_slice(output)
        .map_err(|e| format!("could not parse `docker {kind} inspect` output: {e}"))?;
    validate_inspect_facts(facts, kind)
}

fn validate_inspect_facts(facts: Value, kind: &str) -> Result<Value> {
    let is_valid = facts
        .as_array()
        .and_then(|items| items.first())
        .map(Value::is_object)
        .unwrap_or(false);

    if is_valid {
        Ok(facts)
    } else {
        Err(format!(
            "unexpected `docker {kind} inspect` output: expected a non-empty JSON array"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_container() -> Value {
        serde_json::json!([{
            "Name": "/runlike_fixture1",
            "Image": "sha256:deadbeef",
            "Config": {
                "Hostname": "Essos",
                "User": "daemon",
                "MacAddress": "",
                "Image": "runlike_fixture",
                "Env": [
                    "FOO=thing=\"quoted value with 'spaces' and 'single quotes'\"",
                    "SET_WITHOUT_VALUE",
                    "UTF_8=ユーザー別サイト",
                    "IMAGE_ENV=from-image"
                ],
                "Entrypoint": null,
                "Cmd": ["/bin/bash", "sleep.sh"],
                "Labels": {
                    "com.example.environment": "test",
                    "com.example.notescaped": "$KEEP_DOLLAR",
                    "ImageLabel": "from-image"
                },
                "AttachStdout": false,
                "Tty": true,
                "WorkingDir": "/workdir"
            },
            "HostConfig": {
                "PortBindings": {
                    "300/tcp": [{"HostIp": "", "HostPort": ""}],
                    "400/tcp": [{"HostIp": "", "HostPort": "400"}],
                    "301/udp": [{"HostIp": "", "HostPort": ""}],
                    "502/udp": [{"HostIp": "", "HostPort": "503"}],
                    "1000/tcp": null,
                    "1000/udp": null
                },
                "Links": [
                    "runlike_fixture1:/runlike_fixture5/runlike_fixture1",
                    "runlike_fixture4:/runlike_fixture5/alias_of4"
                ],
                "PidMode": "",
                "CpusetCpus": "",
                "CpusetMems": "",
                "RestartPolicy": {"Name": "always", "MaximumRetryCount": 0},
                "Devices": [
                    {"PathOnHost": "/dev/null", "PathInContainer": "/dev/null", "CgroupPermissions": "r"}
                ],
                "LogConfig": {"Type": "fluentd", "Config": {"tag": "docker.runlike"}},
                "ExtraHosts": ["hostname2:127.0.0.2"],
                "Runtime": "runc",
                "ShmSize": 536870912,
                "Memory": 2147483648u64,
                "MemoryReservation": 1610612736u64,
                "AutoRemove": false,
                "NetworkMode": "default",
                "Privileged": true,
                "VolumesFrom": null,
                "CapAdd": ["CHOWN"],
                "CapDrop": null,
                "Dns": ["8.8.8.8", "8.8.4.4"]
            },
            "NetworkSettings": {
                "Ports": {
                    "1000/tcp": null,
                    "1000/udp": null,
                    "301/udp": [{"HostIp": "", "HostPort": "301"}]
                },
                "Networks": {}
            },
            "Mounts": [
                {
                    "Type": "bind",
                    "Source": "/some/dir",
                    "Destination": "/workdir",
                    "RW": true
                },
                {
                    "Type": "bind",
                    "Source": "/some/dir",
                    "Destination": "/workdir_ro",
                    "RW": false
                },
                {
                    "Type": "volume",
                    "Name": "test_volume",
                    "Destination": "/test_volume",
                    "RW": true
                }
            ]
        }])
    }

    fn fixture_image() -> Value {
        serde_json::json!([{
            "Config": {
                "Env": ["IMAGE_ENV=from-image"],
                "Labels": {"ImageLabel": "from-image"},
                "Entrypoint": null
            }
        }])
    }

    fn make_inspector(pretty: bool) -> Inspector {
        let mut inspector = Inspector::new(
            Some("runlike_fixture1".to_string()),
            false,
            false,
            pretty,
            false,
        );
        inspector.container_facts = Some(fixture_container());
        inspector.image_facts = Some(fixture_image());
        inspector
    }

    #[test]
    fn formats_expected_options() {
        let output = make_inspector(true).format_cli().unwrap();

        assert!(output.starts_with("docker run "));
        assert!(output.contains("--name=runlike_fixture1 \\"));
        assert!(output.contains("--hostname=Essos \\"));
        assert!(output.contains("--user=daemon \\"));
        assert!(output.contains("-p 300 \\"));
        assert!(output.contains("-p 400:400 \\"));
        assert!(output.contains("-p 301/udp \\"));
        assert!(output.contains("-p 503:502/udp \\"));
        assert!(output.contains("--expose=1000 \\"));
        assert!(output.contains("--expose=1000/udp \\"));
        assert!(output.contains("--dns=8.8.8.8 \\"));
        assert!(output.contains("--dns=8.8.4.4 \\"));
        assert!(output.contains("--volume /some/dir:/workdir \\"));
        assert!(output.contains("--volume /some/dir:/workdir_ro:ro \\"));
        assert!(output.contains("--volume /test_volume \\"));
        assert!(output.contains("--env=SET_WITHOUT_VALUE \\"));
        assert!(output.contains("--env='UTF_8=ユーザー別サイト' \\"));
        assert!(!output.contains("--env=IMAGE_ENV=from-image"));
        assert!(output.contains("--cap-add=CHOWN \\"));
        assert!(output.contains("--privileged \\"));
        assert!(output.contains("--workdir=/workdir \\"));
        assert!(output.contains("--link runlike_fixture1 \\"));
        assert!(output.contains("--link runlike_fixture4:alias_of4 \\"));
        assert!(output.contains("--restart=always \\"));
        assert!(output.contains("--device /dev/null:/dev/null:r \\"));
        assert!(output.contains("--label='com.example.environment=test' \\"));
        assert!(output.contains("--label='com.example.notescaped=$KEEP_DOLLAR' \\"));
        assert!(!output.contains("--label='ImageLabel"));
        assert!(output.contains("--log-driver=fluentd \\"));
        assert!(output.contains("--log-opt tag=docker.runlike \\"));
        assert!(output.contains("--add-host hostname2:127.0.0.2 \\"));
        assert!(output.contains("--runtime=runc \\"));
        assert!(output.contains("--shm-size=\"536870912\" \\"));
        assert!(output.contains("--memory=\"2147483648\" \\"));
        assert!(output.contains("--memory-reservation=\"1610612736\" \\"));
        assert!(output.contains("--detach=true \\"));
        assert!(output.contains("-t \\"));
        assert!(output.contains("runlike_fixture \\"));
        assert!(output.ends_with("/bin/bash sleep.sh"));
    }

    #[test]
    fn can_disable_name_and_labels() {
        let mut inspector = Inspector::new(
            Some("runlike_fixture1".to_string()),
            true,
            false,
            false,
            true,
        );
        inspector.container_facts = Some(fixture_container());
        inspector.image_facts = Some(fixture_image());

        let output = inspector.format_cli().unwrap();
        assert!(!output.contains("--name"));
        assert!(!output.contains("--label"));
        assert!(output.starts_with("docker run --hostname=Essos "));
    }

    #[test]
    fn can_keep_volume_ids() {
        let mut inspector = Inspector::new(
            Some("runlike_fixture1".to_string()),
            false,
            true,
            false,
            false,
        );
        inspector.container_facts = Some(fixture_container());
        inspector.image_facts = Some(fixture_image());

        let output = inspector.format_cli().unwrap();
        assert!(output.contains("--volume test_volume:/test_volume"));
    }

    #[test]
    fn quotes_command_parts() {
        let mut container = fixture_container();
        container[0]["Config"]["Cmd"] = serde_json::json!(["bash", "-c", "bash 'sleep.sh'"]);
        let mut inspector = Inspector::new(None, false, false, false, false);
        inspector.container_facts = Some(container);
        inspector.image_facts = Some(fixture_image());

        let output = inspector.format_cli().unwrap();
        assert!(output.ends_with("bash -c 'bash \\'sleep.sh\\''"));
    }

    #[test]
    fn stdin_mode_has_no_image_facts() {
        let mut inspector = Inspector::new(None, false, false, false, false);
        inspector
            .set_container_facts(&fixture_container().to_string())
            .unwrap();

        let output = inspector.format_cli().unwrap();
        // Without image facts, image-provided values are emitted.
        assert!(output.contains("--env=IMAGE_ENV=from-image"));
        assert!(output.contains("--label='ImageLabel=from-image'"));
    }
}
