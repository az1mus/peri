use agent_client_protocol::schema::{
    AgentCapabilities, Implementation, InitializeRequest, InitializeResponse,
};
use agent_client_protocol::{Client, ConnectionTo, Responder};

pub async fn handle_initialize(
    req: InitializeRequest,
    responder: Responder<InitializeResponse>,
    _conn: ConnectionTo<Client>,
) -> Result<(), agent_client_protocol::Error> {
    let mut caps = AgentCapabilities::default();
    caps.load_session = true;
    caps.prompt_capabilities.image = true;
    caps.session_capabilities.close = Some(Default::default());
    caps.session_capabilities.list = Some(Default::default());
    caps.session_capabilities.resume = Some(Default::default());

    let agent_info =
        Implementation::new("perihelion", env!("CARGO_PKG_VERSION")).title("Perihelion Agent");

    let response = InitializeResponse::new(req.protocol_version)
        .agent_capabilities(caps)
        .agent_info(agent_info);

    let _ = responder.respond(response);
    Ok(())
}
