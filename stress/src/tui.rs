use ratatui::{
    Frame,
    layout::Alignment,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, BarChart, Block, Borders, Chart, Dataset, GraphType, Paragraph, Wrap},
};
use std::collections::VecDeque;

pub struct AppState {
    pub latencies: Vec<(String, u64)>,

    pub distribution_data: Vec<(String, u64)>,

    pub users_history: Vec<(f64, f64)>,

    pub pings_history: Vec<(f64, f64)>,

    pub current_time: f64,

    pub log_counts: LogCounts,

    pub logs: VecDeque<String>,
}

#[derive(Default)]
pub struct LogCounts {
    pub error: usize,
    pub warn: usize,
    pub info: usize,
    pub debug: usize,
}

fn format_number(num: u64) -> String {
    if num >= 1_000_000 {
        let formatted = format!("{:.1}", num as f64 / 1_000_000.0);
        if formatted.ends_with(".0") {
            format!("{}M", &formatted[..formatted.len() - 2])
        } else {
            format!("{}M", formatted)
        }
    } else if num >= 1_000 {
        let formatted = format!("{:.1}", num as f64 / 1_000.0);
        if formatted.ends_with(".0") {
            format!("{}K", &formatted[..formatted.len() - 2])
        } else {
            format!("{}K", formatted)
        }
    } else {
        num.to_string()
    }
}

pub fn draw_tui(f: &mut Frame, app: &AppState) {
    let size = f.area();

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(size);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[0]);

    let middle_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[1]);

    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
        .split(main_chunks[2]);

    let bar_data: Vec<(&str, u64)> = app
        .latencies
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();

    let barchart = BarChart::default()
        .block(
            Block::default()
                .title(Span::styled(
                    " Latencies ",
                    Style::default().fg(Color::Cyan),
                ))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .data(&bar_data)
        .bar_width(6)
        .bar_gap(1)
        .value_style(Style::default().fg(Color::White).bg(Color::Blue))
        .label_style(Style::default().fg(Color::White))
        .bar_style(Style::default().fg(Color::Blue));

    f.render_widget(barchart, top_chunks[0]);

    let dist_data: Vec<(&str, u64)> = app
        .distribution_data
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();

    let dist_barchart = BarChart::default()
        .block(
            Block::default()
                .title(Span::styled(
                    " Latency Distribution ",
                    Style::default().fg(Color::Magenta),
                ))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .data(&dist_data)
        .bar_width(6)
        .bar_gap(1)
        .value_style(Style::default().fg(Color::White).bg(Color::Magenta))
        .label_style(Style::default().fg(Color::White))
        .bar_style(Style::default().fg(Color::Magenta));

    f.render_widget(dist_barchart, top_chunks[1]);

    let x_max = app.current_time;
    let x_min = (app.current_time - 60.0).max(0.0);

    let y_max_users = app
        .users_history
        .iter()
        .map(|&(_, y)| y as u64)
        .max()
        .unwrap_or(10) as f64
        * 1.2;

    let user_datasets = vec![
        Dataset::default()
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .data(&app.users_history),
    ];

    let users_chart = Chart::new(user_datasets)
        .block(
            Block::default()
                .title(Span::styled(
                    " Active Users ",
                    Style::default().fg(Color::Cyan),
                ))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .x_axis(
            Axis::default()
                .title(Span::styled(" X ", Style::default().fg(Color::Gray)))
                .style(Style::default().fg(Color::Gray))
                .bounds([x_min, x_max])
                .labels(vec![
                    Line::from(Span::styled("-60s", Style::default().fg(Color::Gray))),
                    Line::from(Span::styled("-30s", Style::default().fg(Color::Gray))),
                    Line::from(Span::styled("0s", Style::default().fg(Color::Gray))),
                ]),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled(" Y ", Style::default().fg(Color::Gray)))
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, y_max_users.max(10.0)])
                .labels(vec![
                    Line::from(Span::raw("0")),
                    Line::from(Span::raw(format_number(y_max_users as u64 / 2))),
                    Line::from(Span::raw(format_number(y_max_users as u64))),
                ]),
        );

    f.render_widget(users_chart, middle_chunks[0]);

    let y_max_pings = app
        .pings_history
        .iter()
        .map(|&(_, y)| y as u64)
        .max()
        .unwrap_or(10) as f64
        * 1.2;

    let ping_datasets = vec![
        Dataset::default()
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Cyan))
            .graph_type(GraphType::Line)
            .data(&app.pings_history),
    ];

    let pings_chart = Chart::new(ping_datasets)
        .block(
            Block::default()
                .title(Span::styled(
                    " Total Pings ",
                    Style::default().fg(Color::Cyan),
                ))
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .x_axis(
            Axis::default()
                .title(Span::styled(" X ", Style::default().fg(Color::Gray)))
                .style(Style::default().fg(Color::Gray))
                .bounds([x_min, x_max])
                .labels(vec![
                    Line::from(Span::styled("-60s", Style::default().fg(Color::Gray))),
                    Line::from(Span::styled("-30s", Style::default().fg(Color::Gray))),
                    Line::from(Span::styled("0s", Style::default().fg(Color::Gray))),
                ]),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled(" Y ", Style::default().fg(Color::Gray)))
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, y_max_pings.max(10.0)])
                .labels(vec![
                    Line::from(Span::raw("0")),
                    Line::from(Span::raw(format_number(y_max_pings as u64 / 2))),
                    Line::from(Span::raw(format_number(y_max_pings as u64))),
                ]),
        );

    f.render_widget(pings_chart, middle_chunks[1]);

    let log_counts_text = vec![
        Line::from(vec![
            Span::styled("ERROR: ", Style::default().fg(Color::Red).bold()),
            Span::raw(format_number(app.log_counts.error as u64)),
        ]),
        Line::from(vec![
            Span::styled("WARN : ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(format_number(app.log_counts.warn as u64)),
        ]),
        Line::from(vec![
            Span::styled("INFO : ", Style::default().fg(Color::Green).bold()),
            Span::raw(format_number(app.log_counts.info as u64)),
        ]),
        Line::from(vec![
            Span::styled("DEBUG: ", Style::default().fg(Color::DarkGray).bold()),
            Span::raw(format_number(app.log_counts.debug as u64)),
        ]),
    ];

    let log_counts_paragraph = Paragraph::new(log_counts_text).block(
        Block::default()
            .title(" System Logs ")
            .borders(Borders::ALL),
    );

    f.render_widget(log_counts_paragraph, bottom_chunks[0]);

    let logs_text: Vec<Line> = app.logs.iter().map(|l| Line::from(l.as_str())).collect();

    let logs_paragraph = Paragraph::new(logs_text)
        .block(Block::default().title(" Logs ").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    f.render_widget(logs_paragraph, bottom_chunks[1]);
}
