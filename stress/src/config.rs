pub struct Config {
    pub api_url: String,
    pub addr: String,
    pub users_to_create: usize,
    pub concurrency: usize,
    pub local_ips: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut local_ips = Vec::new();
        for i in 2..=200 {
            local_ips.push(format!("127.0.0.{}", i));
        }

        Self {
            api_url: "http://localhost:8080".to_string(),
            addr: "127.0.0.1:8081".to_string(),
            users_to_create: 1_000_000,
            concurrency: 10_000,
            local_ips,
        }
    }
}
