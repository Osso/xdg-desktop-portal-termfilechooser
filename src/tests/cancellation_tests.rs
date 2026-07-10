use super::*;

#[test]
fn stops_running_terminal_process() {
    let terminal = executable_script("terminal", "#!/bin/sh\nsleep 30\n");
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    let chooser = FileChooser::new(config);
    let cancellation = CancellationToken::new();
    let running = chooser
        .start_terminal(
            "ignored",
            &["true".into(), "ignored".into()],
            cancellation.clone(),
        )
        .unwrap();

    cancellation.cancel();
    let started = std::time::Instant::now();
    let error = zbus::block_on(running.wait()).unwrap_err();

    assert_eq!(error, "Request cancelled");
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn stops_terminal_descendants() {
    let child_pid_file = tempfile::NamedTempFile::new().unwrap();
    let script = format!(
        "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
        child_pid_file.path().display()
    );
    let terminal = executable_script("terminal", &script);
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    let chooser = FileChooser::new(config);
    let cancellation = CancellationToken::new();
    let running = chooser
        .start_terminal(
            "ignored",
            &["true".into(), "ignored".into()],
            cancellation.clone(),
        )
        .unwrap();

    let child_pid = (0..50)
        .find_map(|_| {
            let content = std::fs::read_to_string(child_pid_file.path()).ok()?;
            let pid = content.trim().parse::<u32>().ok();
            if pid.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            pid
        })
        .expect("terminal did not start descendant");
    cancellation.cancel();
    let _ = zbus::block_on(running.wait());
    let descendant_path = PathBuf::from(format!("/proc/{child_pid}"));
    let stopped = (0..50).any(|_| {
        if !descendant_path.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        false
    });
    if !stopped {
        let _ = Command::new("kill").arg(child_pid.to_string()).status();
    }

    assert!(
        stopped,
        "chooser descendant {child_pid} survived cancellation"
    );
}

#[test]
fn portal_request_close_cancels_interaction() {
    let cancellation = CancellationToken::new();
    let request = runtime::PortalRequest::new(cancellation.clone());

    zbus::block_on(request.close());

    assert!(cancellation.is_cancelled());
}

#[test]
fn request_close_ends_active_open_file_as_cancellation() {
    let terminal = executable_script("terminal", "#!/bin/sh\nsleep 30\n");
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    let chooser = FileChooser::new(config);
    let cancellation = CancellationToken::new();
    let request = runtime::PortalRequest::new(cancellation.clone());
    let closer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        zbus::block_on(request.close());
    });

    let (code, result) = zbus::block_on(chooser.open_file_result_with_cancellation(
        "Open",
        HashMap::new(),
        cancellation,
    ));
    closer.join().unwrap();

    assert_eq!(code, 1);
    assert!(result.is_empty());
}
