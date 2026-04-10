#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct IndexerConfig {
    pub override_from: Option<String>,
    /// A path to the file that stores your node identity and secret key
    pub identity_file: PathBuf,
    /// The relative path to store persistent data
    pub data_dir: PathBuf,
    /// The p2p configuration settings
    pub p2p: P2pConfig,
    /// Listening address for the indexer API server
    pub api_listen_address: Option<SocketAddr>,
    /// GraphQL port of the indexer application
    pub graphql_address: Option<SocketAddr>,
    /// The address of the Web UI
    pub web_ui_address: Option<SocketAddr>,
    /// The publicly-accessible URL that the UI uses to connect to the API.
    pub web_ui_public_api_url: Option<String>,
    /// The jrpc address where the UI should connect to the GraphQL API
    pub web_ui_public_graphql_url: Option<String>,
    /// How often do we want to scan the second layer for new versions
    #[serde(with = "serializers::seconds")]
    pub block_scanning_interval: Duration,
    #[serde(with = "serializers::seconds")]
    pub state_scanning_interval: Duration,
    /// Rate limiting configurations
    pub rate_limits: RateLimits,
    // Other existing configurations...
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RateLimits {
    pub post_transactions: usize,
    pub post_substates_fetch: usize,
    pub post_utxos_fetch: usize,
    pub get_non_fungibles: usize,
    pub sse_max_connections: usize,
}