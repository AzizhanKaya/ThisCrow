use crate::auth::JwtUser;
use crate::models::VoiceChat;
use crate::models::webRTC;
use crate::{State, id};
use actix_web::{Error, HttpResponse, error, web};
use dashmap::DashMap;
use log::{error, info, warn};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio;
use webrtc::api::APIBuilder;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;
use webrtc_ice::network_type;

#[derive(Deserialize, Debug)]
pub struct RTCIceCandidateInitForRest {
    pub candidate: String,
    #[serde(rename = "sdpMid")]
    pub sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex")]
    pub sdp_m_line_index: Option<u16>,
    #[serde(rename = "usernameFragment")]
    pub username_fragment: Option<String>,
}

fn create_media_engine() -> MediaEngine {
    let mut m = MediaEngine::default();
    m.register_default_codecs().unwrap();
    m
}

pub fn init_webrtc_api() -> Arc<webrtc::api::API> {
    let media_engine = create_media_engine();
    let registry = Registry::new();

    let mut setting_engine = SettingEngine::default();

    setting_engine.set_network_types(vec![network_type::NetworkType::Udp6]);

    Arc::new(
        APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build(),
    )
}

pub async fn attach_rtc_handler(
    user: web::ReqData<JwtUser>,
    chat_id: web::Path<id>,
    state: State,
) -> Result<HttpResponse, Error> {
    let user_id = user.id;
    let chat_id = chat_id.into_inner();

    let voice_chat_entry = state.chats.entry(chat_id).or_insert_with(|| VoiceChat {
        id: chat_id,
        users: DashMap::new(),
    });

    let voice_chat = voice_chat_entry.value();

    if voice_chat.users.contains_key(&user_id) {
        return Ok(HttpResponse::Conflict().body("User already in RTC session"));
    }

    let config = RTCConfiguration {
        ice_transport_policy: RTCIceTransportPolicy::All,
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let state_ice = state.clone();

    match state.rtc_api.new_peer_connection(config).await {
        Ok(pc) => {
            pc.on_ice_candidate(Box::new(move |ice_candidate| {
                let state_ice = state_ice.clone();
                Box::pin(async move {
                    if let Some(candidate) = ice_candidate {
                        let candidate_json = candidate.to_json().unwrap();
                        if let Some(mut user) = state_ice.users.get_mut(&user_id) {
                            if let Err(e) = user
                                .ws_conn
                                .text(
                                    json!({
                                        "type": "ice-candidate",
                                        "data": candidate_json
                                    })
                                    .to_string(),
                                )
                                .await
                            {
                                warn!("Failed to send ICE candidate: {:?}", e);
                            }
                        }
                    } else {
                        info!("ICE candidate: None (All candidates sent)");
                    }
                })
            }));

            let rtc_model = webRTC {
                peer_connection: pc,
            };

            voice_chat.users.insert(user_id, rtc_model);
            Ok(HttpResponse::Ok().body("RTC attached"))
        }
        Err(_) => Err(error::ErrorInternalServerError(
            "Failed to create RTC PeerConnection",
        )),
    }
}

pub async fn process_offer(
    user_id: id,
    chat_id: id,
    offer: serde_json::Value,
    state: State,
) -> Result<serde_json::Value, Error> {
    let offer = RTCSessionDescription::offer(
        offer
            .get("sdp")
            .and_then(|v| v.as_str())
            .ok_or_else(|| error::ErrorBadRequest("SDP field not found or not a string"))?
            .to_string(),
    )
    .map_err(|_| error::ErrorBadRequest("Malformed SDP offer provided"))?;

    let voice_chat = state
        .chats
        .get(&chat_id)
        .ok_or_else(|| error::ErrorNotFound("Voice chat not found"))?;

    let user_rtc_session = voice_chat
        .users
        .get(&user_id)
        .ok_or_else(|| error::ErrorConflict("User not in voice chat"))?;

    let pc = &user_rtc_session.peer_connection;

    let track_state = state.clone();

    pc.on_track(Box::new(move |track, _, _| {
        let state = track_state.clone();

        Box::pin(async move {
            if track.kind() == RTPCodecType::Audio {
                tokio::spawn(relay_track_to_other_users(state, chat_id, user_id, track));
            }
        })
    }));

    let state = state.clone();

    pc.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        let state = state.clone();
        Box::pin(async move {
            match s {
                RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Closed => {
                    if let Some(chat) = state.chats.get_mut(&chat_id) {
                        chat.users.remove(&user_id);
                        if chat.users.is_empty() {
                            state.chats.remove(&chat_id);
                        }
                    }
                }
                _ => {}
            }
        })
    }));

    if let Err(e) = pc.set_remote_description(offer).await {
        error!("Failed to set remote description: {}", e);
        return Err(error::ErrorInternalServerError(format!(
            "Failed to set remote description {e}"
        )));
    }

    let answer = pc.create_answer(None).await.map_err(|e| {
        error!("Failed to create answer: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to create answer")
    })?;

    pc.set_local_description(answer.clone())
        .await
        .map_err(|e| {
            error!("Failed to set local description: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to set local description")
        })?;

    Ok(json!({
        "type": "answer",
        "sdp": answer.sdp
    }))
}

pub async fn process_ice_candidate(
    user: web::ReqData<JwtUser>,
    path: web::Path<id>,
    candidate_init_rest: web::Json<RTCIceCandidateInitForRest>,
    state: State,
) -> Result<HttpResponse, Error> {
    let user_id = user.id;
    let chat_id = path.into_inner();

    let voice_chat = state
        .chats
        .get(&chat_id)
        .ok_or_else(|| error::ErrorNotFound(format!("Voice chat {} not found", chat_id)))?;

    let user_rtc_session = voice_chat
        .users
        .get(&user_id)
        .ok_or_else(|| {
            error::ErrorNotFound(format!("User {} not found in chat {}", user_id, chat_id))
        })
        .unwrap();

    let pc = &user_rtc_session.peer_connection;

    let rtc_candidate_init = RTCIceCandidateInit {
        candidate: candidate_init_rest.candidate.clone(),
        sdp_mid: candidate_init_rest.sdp_mid.clone(),
        sdp_mline_index: candidate_init_rest.sdp_m_line_index,
        username_fragment: candidate_init_rest.username_fragment.clone(),
    };

    match pc.add_ice_candidate(rtc_candidate_init).await {
        Ok(_) => Ok(HttpResponse::Ok().finish()),
        Err(e) => {
            error!("ICE candidate ekleme hatası: {:?}", e);
            Err(error::ErrorInternalServerError(format!(
                "Failed to add ICE candidate: {e}"
            )))
        }
    }
}

pub async fn process_answer(
    user_id: id,
    chat_id: id,
    answer: serde_json::Value,
    state: State,
) -> Result<serde_json::Value, Error> {
    let answer = RTCSessionDescription::answer(
        answer
            .get("sdp")
            .and_then(|v| v.as_str())
            .ok_or_else(|| error::ErrorBadRequest("SDP field not found or not a string"))?
            .to_string(),
    )
    .map_err(|_| error::ErrorBadRequest("Malformed SDP answer provided"))?;

    let voice_chat = state
        .chats
        .get(&chat_id)
        .ok_or_else(|| error::ErrorNotFound("Voice chat not found"))?;

    let user_rtc_session = voice_chat
        .users
        .get(&user_id)
        .ok_or_else(|| error::ErrorConflict("User not in voice chat"))?;

    let pc = &user_rtc_session.peer_connection;

    if let Err(e) = pc.set_remote_description(answer).await {
        error!("Failed to set remote description: {}", e);
        return Err(error::ErrorInternalServerError(format!(
            "Failed to set remote description {e}"
        )));
    }

    Ok(json!({
        "type": "success",
        "data": "answer işlendi"
    }))
}

async fn relay_track_to_other_users(
    state: State,
    chat_id: id,
    source_user_id: id,
    incoming_track: Arc<TrackRemote>,
) {
    let voice_chat = match state.chats.get(&chat_id) {
        Some(vc) => vc,
        None => {
            warn!(
                "Chat ID '{}' bulunamadı, track relay iptal edildi.",
                chat_id
            );
            return;
        }
    };

    let mut outgoing_tracks: HashMap<id, Arc<TrackLocalStaticRTP>> = HashMap::new();

    let incoming_codec_params = incoming_track.codec();
    let current_track_capability: RTCRtpCodecCapability = incoming_codec_params.capability.clone();

    for user_entry in voice_chat.users.iter() {
        let target_user_id = *user_entry.key();

        if target_user_id == source_user_id {
            continue;
        }

        let user_rtc_session = user_entry.value();
        let pc = &user_rtc_session.peer_connection;

        let relayed_track_id = format!(
            "relay-{}-from-{}-to-{}",
            incoming_track.id(),
            source_user_id,
            target_user_id
        );
        let relayed_stream_id = format!(
            "stream-relay-{}-from-{}-to-{}",
            incoming_track.stream_id(),
            source_user_id,
            target_user_id
        );

        let local_track_for_target = Arc::new(TrackLocalStaticRTP::new(
            current_track_capability.clone(),
            relayed_track_id.clone(),
            relayed_stream_id.clone(),
        ));

        match pc.add_track(local_track_for_target.clone()).await {
            Ok(_rtp_sender) => {
                let offer = pc
                    .create_offer(None)
                    .await
                    .map_err(|e| {
                        error!("Offer oluşturma hatası: {:?}", e);
                        e
                    })
                    .unwrap();

                pc.set_local_description(offer.clone())
                    .await
                    .map_err(|e| {
                        error!("Local description set hatası: {:?}", e);
                        e
                    })
                    .unwrap();

                let offer = json!({
                    "type": "offer",
                    "sdp": offer.sdp
                });

                if let Some(mut user) = state.users.get_mut(&target_user_id) {
                    if let Err(e) = user.ws_conn.text(offer.to_string()).await {
                        warn!("Failed to send offer track: {:?}", e);
                    }
                }

                outgoing_tracks.insert(target_user_id, local_track_for_target);
            }
            Err(e) => {
                error!("Track ekleme hatası: {:?}", e);
            }
        }
    }

    loop {
        match incoming_track.read_rtp().await {
            Ok((rtp_packet, _)) => {
                let mut write_futures = Vec::new();

                for (target_user_id_ref, local_track) in outgoing_tracks.iter() {
                    let target_user_id = *target_user_id_ref;

                    let fut = local_track.write_rtp(&rtp_packet);

                    write_futures.push(async move {
                        match fut.await {
                            Ok(_) => {}
                            Err(e) => {
                                if e == webrtc::Error::ErrClosedPipe {
                                    warn!(
                                        "Closed pipe | Target User: {}, Hata: {:?}",
                                        target_user_id, e
                                    );
                                } else {
                                    error!(
                                        "RTP yazma hatası | Target User: {}, Hata: {:?}",
                                        target_user_id, e
                                    );
                                }
                            }
                        }
                    });
                }
                futures::future::join_all(write_futures).await;
            }
            Err(e) => {
                if e == webrtc::Error::ErrClosedPipe {
                } else {
                    error!(
                        "Track read_rtp hatası, relay döngüsü sonlandırılıyor | Chat ID: {}, Source User: {}, Hata: {:?}",
                        chat_id, source_user_id, e
                    );
                }
                break;
            }
        }
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/rtc")
            .route("/attach/{chat_id}", web::post().to(attach_rtc_handler))
            .route(
                "/candidate/{chat_id}",
                web::post().to(process_ice_candidate),
            ),
    );
}
