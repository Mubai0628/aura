use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelDiscoveryMode, ProviderPresetParameterKind,
};

/// A Provider as Aura knows how to reach it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderProfile {
    /// Aura's own name for the profile.
    pub profile_id: String,
    /// The catalog Providers whose connections start from this profile's routes.
    pub catalog_provider_ids: Vec<String>,
    pub route_choice: RouteChoice,
    pub routes: Vec<ProviderRoute>,
    /// What requests to the profile's hosts carry beyond the protocol.
    pub requests: Vec<RequestRule>,
}

/// How a profile's routes divide the Provider's models.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteChoice {
    /// Each route takes any of the models; a connection goes by the one the user picks.
    OneOf,
    /// Each model goes by the route that serves its catalog route, so the Provider is reached
    /// through one connection of each.
    ByModel,
}

/// One way to reach a Provider: a protocol at a base URL, the key it takes, and the defaults a
/// connection made from it starts with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRoute {
    pub route_id: String,
    pub display_name: String,
    /// With a `{name}` standing for each of `parameters`.
    pub base_url: String,
    pub api_format: ModelApiFormat,
    pub auth_scheme: EndpointAuthScheme,
    pub discovery_mode: ModelDiscoveryMode,
    pub chat_instruction_role: Option<ChatInstructionRole>,
    pub chat_output_token_field: Option<ChatOutputTokenField>,
    /// The route a connection to the Provider starts from unless the user picks another.
    pub recommended: bool,
    /// The catalog routes this route serves, in a profile whose models go by their own routes:
    /// the catalog's package and address only select a route, never reach a request.
    pub catalog_routes: Vec<CatalogRoute>,
    /// The values of the user's own the base URL takes.
    pub parameters: Vec<RouteParameter>,
}

/// A non-secret value of the user's own, such as an account or a region, that stands for one
/// whole label of the base URL's host or one whole segment of its path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteParameter {
    /// As it stands in the base URL, between braces.
    pub name: String,
    pub display_name: String,
    pub kind: ProviderPresetParameterKind,
    pub example: String,
}

/// A route as the catalog names it, for a model or, where the model names none, its Provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogRoute {
    pub npm_package: String,
    pub api_url: String,
    /// `responses` or `completions` within an OpenAI-style package; `None` where the catalog
    /// gives none.
    pub shape: Option<String>,
}

/// The rules for the requests a target names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestRule {
    pub target: RequestTarget,
    pub rules: RequestRules,
}

/// Requests by scheme, host and protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestTarget {
    /// `None` for any scheme.
    pub scheme: Option<String>,
    pub host: HostPattern,
    /// Empty for every protocol.
    pub api_formats: Vec<ModelApiFormat>,
}

/// Hosts, as the URL parser gives them: in lower case, and with a trailing dot naming the same
/// host as without it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostPattern {
    /// This host only.
    Exact(String),
    /// This domain and every name under it.
    Domain(String),
    /// `localhost` or a loopback address: a gateway on this computer.
    Loopback,
}

/// What a request carries beyond the protocol.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestRules {
    pub routing_key: Option<RoutingKey>,
    /// User-Agent names Aura and its version instead of the HTTP library.
    pub aura_user_agent: bool,
    /// The task's current state goes before the conversation instead of after it, where a
    /// prefix cache keeps more of a turn's steps (Responses).
    pub state_before_conversation: bool,
    /// The body marks the stable instructions and the history's tail as cache breakpoints
    /// (Anthropic Messages' `cache_control`), which compatible endpoints may reject.
    pub cache_breakpoints: bool,
    /// The Provider is asked to keep neither the request nor its reply (Responses' `store:
    /// false`), so a conversation goes on only by what each request carries: reasoning goes back
    /// only as the Provider returned it encrypted, never by an id it would look up.
    pub store_off: bool,
    /// A model that reasons is asked for its reasoning encrypted (Responses' `include`), as a
    /// conversation the Provider keeps nothing of carries it. A Provider that does not document
    /// it is not asked, and its reasoning does not go back where it keeps nothing.
    pub encrypted_reasoning: bool,
}

/// A key the Provider routes requests by, so that the steps of one conversation reach the same
/// cache. Neither form shows the Provider an Aura record id or the creation time a ULID holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutingKey {
    /// A header whose value stands for the conversation, or for the request itself when it
    /// belongs to no conversation, the same for each attempt.
    ConversationHeader { name: String },
    /// A body field whose value stands for the stable instructions and the conversation's first
    /// item: the same from step to step until a compaction replaces the history.
    PromptPrefixField { name: String },
}
