use crate::auth::JwtUser;
use crate::models::VoiceChat;
use crate::models::webRTC;
use crate::{State, id};
use actix_web::{Error, HttpResponse, error, web};
use dashmap::DashMap;
use futures::lock::Mutex;
use futures::stream;
use futures_util::StreamExt;
use log::{error, info, warn};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::usize;
use tokio;
use uuid::Uuid;
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

    setting_engine.set_network_types(vec![network_type::NetworkType::Udp4]);

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

    if let Some(voice_chat) = state.chats.get(&chat_id) {
        if voice_chat.users.contains_key(&user_id) {
            warn!(
                "User {} already in RTC session for chat {}, cleaning up...",
                user_id, chat_id
            );
            drop(voice_chat);
            cleanup_user_from_chat(state.clone(), chat_id, user_id).await;
            return Err(error::ErrorConflict(
                "User already in RTC session cleaning up...",
            ));
        }
    }

    let config = RTCConfiguration {
        ice_transport_policy: RTCIceTransportPolicy::All,
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let pc = match state.rtc_api.new_peer_connection(config).await {
        Ok(pc) => pc,
        Err(e) => {
            warn!("Failed to create RTC PeerConnection: {:?}", e);
            return Err(error::ErrorInternalServerError(
                "Failed to create RTC PeerConnection",
            ));
        }
    };

    let state_ice = state.clone();

    pc.on_ice_candidate(Box::new(move |ice_candidate| {
        let state = state_ice.clone();
        Box::pin(async move {
            if let Some(candidate) = ice_candidate {
                let candidate_json = candidate.to_json().unwrap();
                if let Some(mut user_conn) = state.users.get_mut(&user_id) {
                    if let Err(e) = user_conn
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

    let voice_chat = state.chats.entry(chat_id).or_insert_with(|| VoiceChat {
        id: uuid::Uuid::try_from("123e4567-e89b-12d3-a456-426614174000").unwrap(),
        users: DashMap::new(),
    });

    voice_chat.users.insert(user_id, rtc_model);

    info!("Created new voice chat session for chat_id: {}", chat_id);

    Ok(HttpResponse::Ok().body("RTC attached"))
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
    .map_err(|e| {
        warn!("Malformed SDP offer provided: {}", e);
        error::ErrorBadRequest("Malformed SDP offer provided")
    })?;

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
                tokio::spawn(relay_track(state, chat_id, user_id, track));
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
                    info!("Cleaning up user {} from chat {}", user_id, chat_id);

                    tokio::spawn(async move {
                        cleanup_user_from_chat(state, chat_id, user_id).await;
                    });
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
    info!("New ice canidate from user: {}", user.id);
    let chat_id = path.into_inner();

    let voice_chat = state
        .chats
        .get(&chat_id)
        .ok_or_else(|| error::ErrorNotFound(format!("Voice chat {} not found", chat_id)))?;

    let user_rtc_session = voice_chat
        .users
        .get(&user.id)
        .ok_or_else(|| {
            error::ErrorNotFound(format!("User {} not found in chat {}", user.id, chat_id))
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

async fn relay_track(
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

    let mut tracks: HashMap<id, Arc<TrackLocalStaticRTP>> = HashMap::new();

    let codec_params = incoming_track.codec();
    let track_capability: RTCRtpCodecCapability = codec_params.capability.clone();

    for user_entry in voice_chat.users.iter() {
        let (&user_id, user_rtc) = user_entry.pair();

        if user_id == source_user_id {
            continue;
        }

        let pc = &user_rtc.peer_connection;

        let track_id = format!("{}-{}-{}", incoming_track.id(), source_user_id, user_id);
        let stream_id = format!(
            "{}-{}-{}",
            incoming_track.stream_id(),
            source_user_id,
            user_id
        );

        let local_track = Arc::new(TrackLocalStaticRTP::new(
            track_capability.clone(),
            track_id,
            stream_id,
        ));

        match pc.add_track(local_track.clone()).await {
            Ok(rtp_sender) => {
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

                if let Some(mut user) = state.users.get_mut(&user_id) {
                    if let Err(e) = user.ws_conn.text(offer.to_string()).await {
                        warn!("Failed to send offer track: {:?}", e);
                        if pc.remove_track(&rtp_sender).await.is_err() {
                            drop(user_entry);
                            tokio::spawn(cleanup_user_from_chat(state.clone(), chat_id, user_id));
                        }
                        continue;
                    }
                }

                tracks.insert(user_id, local_track);
            }
            Err(e) => {
                error!("Track ekleme hatası: {:?}", e);
            }
        }
    }

    drop(voice_chat);
    drop(codec_params);
    drop(track_capability);

    loop {
        match incoming_track.read_rtp().await {
            Ok((rtp_packet, _)) => {
                let rtp_packet = Arc::new(rtp_packet);

                let track_entries: Vec<(id, Arc<TrackLocalStaticRTP>)> =
                    tracks.iter().map(|(k, v)| (*k, Arc::clone(v))).collect();

                let res: Vec<(id, bool)> =
                    stream::iter(track_entries.into_iter().map(|(user_id, local_track)| {
                        let rtp_packet = rtp_packet.clone();
                        async move {
                            match local_track.write_rtp(&rtp_packet).await {
                                Ok(_) => (user_id, false),
                                Err(e) => {
                                    if e == webrtc::Error::ErrClosedPipe {
                                        warn!(
                                            "Closed pipe | Target User: {}, Hata: {:?}",
                                            user_id, e
                                        );
                                        (user_id, true)
                                    } else {
                                        error!(
                                            "RTP yazma hatası | Target User: {}, Hata: {:?}",
                                            user_id, e
                                        );
                                        (user_id, true)
                                    }
                                }
                            }
                        }
                    }))
                    .buffer_unordered(5)
                    .collect()
                    .await;

                if !res.iter().any(|(_, remove)| *remove) {
                    continue;
                }

                let Some(voice_chat) = state.chats.get_mut(&chat_id) else {
                    break;
                };

                for (user_id, remove) in res {
                    if remove {
                        tracks.remove(&user_id);
                        voice_chat.users.remove(&user_id);
                    }
                }
            }
            Err(e) => {
                error!(
                    "Track read_rtp hatası, relay döngüsü sonlandırılıyor | Chat ID: {}, Source User: {}, Hata: {:?}",
                    chat_id, source_user_id, e
                );

                tokio::spawn(cleanup_user_from_chat(
                    state.clone(),
                    chat_id,
                    source_user_id,
                ));

                break;
            }
        }
    }
}

async fn cleanup_user_from_chat(state: State, chat_id: id, user_id: id) {
    let Some(voice_chat) = state.chats.get_mut(&chat_id) else {
        return;
    };

    voice_chat.users.remove(&user_id);

    if voice_chat.users.is_empty() {
        drop(voice_chat);
        state.chats.remove(&chat_id);
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
