use super::filter::{Filter, DEPULICATION_BUFFER_SIZE};
use super::ApplicationState;

use crate::diesel::ExpressionMethods;
use crate::diesel::QueryDsl;
use crate::schema::stations;

use dump_dvb::telegrams::{
    TelegramMetaInformation, 
    r09::R09ReceiveTelegram,
    raw::RawReceiveTelegram
};

use actix_diesel::dsl::AsyncRunQueryDsl;
use actix_web::Responder;
use actix_web::{web, HttpRequest};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use log::{info, warn, error};

use std::sync::RwLock;
use chrono::Utc;

#[derive(Queryable, Debug, Clone)]
pub struct Station {
    pub id: Uuid,
    pub token: Option<String>,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub region: i32,
    pub owner: Uuid,
    pub approved: bool,
    pub deactivated: bool
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    success: bool,
}

// /telegrams/r09/
pub async fn receiving_r09(
    app_state: web::Data<RwLock<ApplicationState>>,
    //filter: web::Data<Arc<RwLock<Filter>>>,
    //sender: web::Data<Arc<(Mutex<DataPipelineSender>, Mutex<DataPipelineSender>)>>,
    //database: web::Data<Arc<Mutex<ClickyBuntyDatabase>>>,
    telegram: web::Json<R09ReceiveTelegram>,
    _req: HttpRequest,
) -> impl Responder {
    info!("[DEBUG] Station: {} Received Telegram: {:?}", &telegram.auth.station, &telegram.data);
    let telegram_hash = Filter::calculate_hash(&*telegram).await;
    // checks if the given telegram is already in the buffer
    let contained = match (*app_state).read() {
        Ok(unlocked) => {
            unlocked.filter.lock().unwrap().last_elements.contains(&telegram_hash)
        }
        Err(_) => {
            // clear poisen and ignore telegram
            //(***app_state).clear_poison();
            true
        }
    };

    if contained {
        return web::Json(Response { success: false });
    }

    // updates the buffer adding the new telegram
    match app_state.write() {
        Ok(writeable_app_state) => {
            let mut writeable_filter  = writeable_app_state.filter.lock().unwrap();
            let index = writeable_filter.iterator;
            writeable_filter.last_elements[index] = telegram_hash;
            writeable_filter.iterator = (writeable_filter.iterator + 1) % DEPULICATION_BUFFER_SIZE;
        }
        Err(_) => {
            //app_state.clear_poison();
            return web::Json(Response { success: false });
        }
    }

    let meta: TelegramMetaInformation;

    if app_state.is_poisoned() {
        //app_state.clear_poison();
    }

    if app_state.read().unwrap().database.lock().unwrap().db.is_some() {
        let station;
        {
            // query database for this station
            match (stations::table
                .filter(stations::id.eq(telegram.auth.station))
                .get_result_async::<Station>(&app_state.write().unwrap().database.lock().unwrap().db.as_ref().unwrap()))
            .await
            {
                Ok(data) => {
                    station = data;
                }
                Err(e) => {
                    error!("Err: {:?}", e);
                    return web::Json(Response { success: false });
                }
            };
        }
        if station.id != telegram.auth.station
            || station.token != Some(telegram.auth.token.clone())
            || !station.approved
            || station.deactivated
        {
            // authentication for telegram failed !
            return web::Json(Response { success: false });
        }
        meta = TelegramMetaInformation {
            time: Utc::now().naive_utc(),
            station: station.id,
            region: station.region,
        };
        info!(
            "[main] Received Telegram! {} {:?}",
            &telegram.auth.station, &telegram
        );

    } else {
        // offline flag is set throw data out unauthenticated
        meta = TelegramMetaInformation {
            time: Utc::now().naive_utc(),
            station: Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap(),
            region: -1
        }
    }

    match app_state.write().unwrap().grpc_sender
        .lock()
        .unwrap()
        .try_send(((*telegram).data.clone(), meta.clone()))
    {
        Err(err) => {
            warn!("[main] Channel GRPC has problems! {:?}", err);
        }
        _ => {}
    }
    match app_state.write().unwrap().database_r09_sender
        .lock()
        .unwrap()
        .try_send((((*telegram).data.clone()), meta))
    {
        Err(err) => {
            warn!("[main] Channel Database has problems! {:?}", err);
        }
        _ => {}
    }

    web::Json(Response { success: true })
}

// /telegrams/raw/
pub async fn receiving_raw(
    app_state: web::Data<RwLock<ApplicationState>>,
    telegram: web::Json<RawReceiveTelegram>,
    _req: HttpRequest,
) -> impl Responder {
    info!("[DEBUG] Station: {} Received Telegram: {:?}", &telegram.auth.station, &telegram.data);
    let telegram_hash = Filter::calculate_hash(&*telegram).await;
    // checks if the given telegram is already in the buffer
    let contained = match (*app_state).read() {
        Ok(unlocked) => {
            unlocked.filter.lock().unwrap().last_elements.contains(&telegram_hash)
        }
        Err(_) => {
            // clear poisen and ignore telegram
            //(***app_state).clear_poison();
            true
        }
    };

    if contained {
        return web::Json(Response { success: false });
    }

    // updates the buffer adding the new telegram
    match app_state.write() {
        Ok(writeable_app_state) => {
            let mut writeable_filter  = writeable_app_state.filter.lock().unwrap();
            let index = writeable_filter.iterator;
            writeable_filter.last_elements[index] = telegram_hash;
            writeable_filter.iterator = (writeable_filter.iterator + 1) % DEPULICATION_BUFFER_SIZE;
        }
        Err(_) => {
            //app_state.clear_poison();
            return web::Json(Response { success: false });
        }
    }

    let meta: TelegramMetaInformation;

    if app_state.is_poisoned() {
        //app_state.clear_poison();
    }

    if app_state.read().unwrap().database.lock().unwrap().db.is_some() {
        let station;
        {
            // query database for this station
            match (stations::table
                .filter(stations::id.eq(telegram.auth.station))
                .get_result_async::<Station>(&app_state.write().unwrap().database.lock().unwrap().db.as_ref().unwrap()))
            .await
            {
                Ok(data) => {
                    station = data;
                }
                Err(e) => {
                    error!("Err: {:?}", e);
                    return web::Json(Response { success: false });
                }
            };
        }
        if station.id != telegram.auth.station
            || station.token != Some(telegram.auth.token.clone())
            || !station.approved
            || station.deactivated
        {
            // authentication for telegram failed !
            return web::Json(Response { success: false });
        }
        meta = TelegramMetaInformation {
            time: Utc::now().naive_utc(),
            station: station.id,
            region: station.region,
        };
        info!(
            "[main] Received Telegram! {} {:?}",
            &telegram.auth.station, &telegram
        );

    } else {
        // offline flag is set throw data out unauthenticated
        meta = TelegramMetaInformation {
            time: Utc::now().naive_utc(),
            station: Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap(),
            region: -1
        }
    }

    match app_state.write().unwrap().database_raw_sender
        .lock()
        .unwrap()
        .try_send((((*telegram).data.clone()), meta))
    {
        Err(err) => {
            warn!("[main] Channel Database has problems! {:?}", err);
        }
        _ => {}
    }

    web::Json(Response { success: true })
}
