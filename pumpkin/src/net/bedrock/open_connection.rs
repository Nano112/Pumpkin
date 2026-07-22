use std::net::SocketAddr;

use pumpkin_protocol::bedrock::{
    MTU, RAKNET_PROTOCOL_VERSION,
    client::raknet::{
        incompatible_protocol::CIncompatibleProtocolVersion,
        open_connection::{COpenConnectionReply1, COpenConnectionReply2},
    },
    server::raknet::open_connection::{SOpenConnectionRequest1, SOpenConnectionRequest2},
};
#[cfg(not(target_family = "wasm"))]
use tokio::net::UdpSocket;
#[cfg(target_family = "wasm")]
use crate::net::wasm_net::UdpSocket;

use crate::{net::bedrock::BedrockClient, server::Server};

impl BedrockClient {
    pub async fn handle_open_connection_1(
        server: &Server,
        packet: SOpenConnectionRequest1,
        addr: SocketAddr,
        socket: &UdpSocket,
    ) {
        if packet.protocol_version != RAKNET_PROTOCOL_VERSION {
            Self::send_offline_packet(
                &CIncompatibleProtocolVersion::new(RAKNET_PROTOCOL_VERSION, server.server_guid),
                addr,
                socket,
            )
            .await;
            return;
        }

        Self::send_offline_packet(
            &COpenConnectionReply1::new(server.server_guid, false, MTU as u16),
            addr,
            socket,
        )
        .await;
    }
    pub async fn handle_open_connection_2(
        server: &Server,
        packet: SOpenConnectionRequest2,
        addr: SocketAddr,
        socket: &UdpSocket,
    ) {
        Self::send_offline_packet(
            &COpenConnectionReply2::new(server.server_guid, addr, packet.mtu, false),
            addr,
            socket,
        )
        .await;
    }
}
