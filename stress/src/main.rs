mod api;
mod config;
mod state;
mod tui;
mod ws;

use api::ApiClient;
use config::Config;

use futures::{StreamExt, TryFutureExt, stream};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (state_inner, latency_rx, log_rx) = state::SharedState::new();
    let shared_state = Arc::new(state_inner);

    let logger = Box::new(state::TuiLogger {
        state: shared_state.clone(),
    });

    log::set_boxed_logger(logger).unwrap();
    log::set_max_level(log::LevelFilter::Info);

    log::info!("Starting stress test...");

    let config = Arc::new(Config::default());

    let local_ips: Vec<IpAddr> = config
        .local_ips
        .iter()
        .map(|s| IpAddr::from_str(s).expect("Invalid IP address in config"))
        .collect();

    let clients = local_ips
        .iter()
        .map(|ip| ApiClient::new(config.api_url.clone(), *ip).unwrap())
        .collect::<Vec<ApiClient>>();

    log::info!("Created {} clients", clients.len());

    let remote_addr = config.addr.parse::<SocketAddr>().unwrap();

    let shared_state_move = shared_state.clone();
    let users_to_create = config.users_to_create;
    let concurrency = config.concurrency;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let (completion_tx, mut completion_rx) = tokio::sync::mpsc::channel::<()>(1);

    log::info!(
        "Starting {} users with {} concurrency",
        users_to_create,
        concurrency
    );

    tokio::spawn(async move {
        let tasks = (0..users_to_create).map(|i| {
            let client = clients[i % clients.len()].clone();

            let local_ip = local_ips[i % local_ips.len()];
            let shared_state = shared_state_move.clone();
            let shutdown_rx = shutdown_rx.clone();
            let completion_tx_task = completion_tx.clone();

            async move {
                match client
                    .login_user(i)
                    .or_else(|_| client.register_user(i))
                    .await
                {
                    Ok(Some(token)) => {
                        tokio::spawn(async move {
                            let _guard = completion_tx_task;
                            if let Err(e) = ws::connect_and_listen(
                                local_ip,
                                remote_addr,
                                token,
                                shared_state.clone(),
                                shutdown_rx,
                            )
                            .await
                            {
                                log::warn!("{}", e);
                            }
                        });
                    }
                    Ok(None) => {
                        log::warn!("User {} registered but no session cookie received", i);
                    }
                    Err(e) => {
                        log::warn!("{}", e);
                    }
                }
            }
        });

        let mut stream = stream::iter(tasks).buffer_unordered(concurrency);

        while let Some(_) = stream.next().await {}
    });

    use crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };
    use ratatui::Terminal;
    use ratatui::backend::CrosstermBackend;
    use std::io;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tui_result = run_tui(&mut terminal, shared_state, latency_rx, log_rx).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = tui_result {
        println!("TUI Error: {}", e);
    }

    let _ = shutdown_tx.send(true);
    println!("Waiting for graceful shutdown (max 5s)...");
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), completion_rx.recv()).await;

    Ok(())
}

async fn run_tui(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    state: Arc<state::SharedState>,
    latency_rx: flume::Receiver<u64>,
    log_rx: flume::Receiver<String>,
) -> anyhow::Result<()> {
    use std::time::Duration;
    use tokio::time::interval;

    let mut tick_rate = interval(Duration::from_millis(250));
    let mut app_state = crate::tui::AppState {
        latencies: vec![],
        users_history: vec![],
        pings_history: vec![],
        current_time: 0.0,
        distribution_data: vec![],
        log_counts: crate::tui::LogCounts::default(),
        logs: std::collections::VecDeque::new(),
    };

    let start_time = std::time::Instant::now();
    let mut local_hist = hdrhistogram::Histogram::<u64>::new_with_bounds(1, 60 * 1000, 3).unwrap();

    loop {
        tokio::select! {
            _ = tick_rate.tick() => {
                while let Ok(latency) = latency_rx.try_recv() {
                    let _ = local_hist.record(latency.max(1));
                }

                let now = start_time.elapsed().as_secs_f64();
                app_state.current_time = now;

                let users = state.active_users.load(std::sync::atomic::Ordering::Relaxed);
                let pings = state.total_pings_sent.load(std::sync::atomic::Ordering::Relaxed);

                let last_time = app_state.users_history.last().map(|&(t, _)| t).unwrap_or(0.0);
                if app_state.users_history.is_empty() || now - last_time >= 1.0 {
                    app_state.users_history.push((now, users as f64));
                    app_state.pings_history.push((now, pings as f64));
                }

                let cutoff = now - 60.0;
                app_state.users_history.retain(|&(t, _)| t >= cutoff);
                app_state.pings_history.retain(|&(t, _)| t >= cutoff);

                app_state.log_counts.error = state.log_error.load(std::sync::atomic::Ordering::Relaxed);
                app_state.log_counts.warn = state.log_warn.load(std::sync::atomic::Ordering::Relaxed);
                app_state.log_counts.info = state.log_info.load(std::sync::atomic::Ordering::Relaxed);
                app_state.log_counts.debug = state.log_debug.load(std::sync::atomic::Ordering::Relaxed);

                while let Ok(log_line) = log_rx.try_recv() {
                    if app_state.logs.len() >= 50 {
                        app_state.logs.pop_front();
                    }
                    app_state.logs.push_back(log_line);
                }

                {
                    if local_hist.is_empty() {
                        app_state.latencies.clear();
                        app_state.distribution_data.clear();
                    } else {
                        app_state.latencies = vec![
                            ("p1".to_string(), local_hist.value_at_percentile(1.0)),
                            ("p10".to_string(), local_hist.value_at_percentile(10.0)),
                            ("p50".to_string(), local_hist.value_at_percentile(50.0)),
                            ("p90".to_string(), local_hist.value_at_percentile(90.0)),
                            ("p95".to_string(), local_hist.value_at_percentile(95.0)),
                            ("p99".to_string(), local_hist.value_at_percentile(99.0)),
                            ("p99.9".to_string(), local_hist.value_at_percentile(99.9)),
                            ("p99.99".to_string(), local_hist.value_at_percentile(99.99)),
                            ("max".to_string(), local_hist.max()),
                        ];

                        let max_val = local_hist.max();
                        let step = (max_val / 10).max(1);
                        let mut dist = vec![];
                        for bucket in local_hist.iter_linear(step) {
                            let count = bucket.count_since_last_iteration();
                            if count > 0 {
                                dist.push((format!("{}ms", bucket.value_iterated_to()), count));
                            }
                        }
                        app_state.distribution_data = dist;
                    }
                }

                terminal.draw(|f| crate::tui::draw_tui(f, &app_state))?;
            }
        }

        if crossterm::event::poll(Duration::from_millis(0))? {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                if key.code == crossterm::event::KeyCode::Char('q')
                    || (key.code == crossterm::event::KeyCode::Char('c')
                        && key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL))
                {
                    break;
                }
            }
        }
    }
    Ok(())
}
