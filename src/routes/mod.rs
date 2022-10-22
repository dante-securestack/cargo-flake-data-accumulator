use super::{ApplicationState, DbPool};
use dump_dvb::management::Station;
use dump_dvb::telegrams::{
    r09::{R09ReceiveTelegram, R09SaveTelegram},
    raw::{RawReceiveTelegram, RawSaveTelegram},
    AuthenticationMeta, TelegramMetaInformation,
};

use actix_web::Responder;
use actix_web::{web, HttpRequest};
use diesel::pg::PgConnection;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};

use chrono::Utc;
use std::sync::Mutex;

#[derive(Serialize, Deserialize)]
pub struct Response {
    success: bool,
}

struct QueryResponse {
    pub telegram_meta: TelegramMetaInformation,
    pub approved: bool,
}

async fn authenticate(conn: &mut PgConnection, auth: &AuthenticationMeta) -> Option<QueryResponse> {
    let station;
    {
        use dump_dvb::schema::stations::dsl::stations;
        use dump_dvb::schema::stations::id;

        match stations.filter(id.eq(auth.station)).first::<Station>(conn) {
            Ok(data) => {
                station = data;
            }
            Err(e) => {
                error!("Err: {:?}", e);
                return None;
            }
        };
    }
    if station.id != auth.station
        || station.token != Some(auth.token.clone())
        || station.deactivated
    {
        // authentication for telegram failed !
        return None;
    }

    Some(QueryResponse {
        telegram_meta: TelegramMetaInformation {
            time: Utc::now().naive_utc(),
            station: station.id,
            region: station.region as i32,
        },
        approved: station.approved,
    })
}

// /telegrams/r09/
pub async fn receiving_r09(
    pool: web::Data<DbPool>,
    app_state: web::Data<Mutex<ApplicationState>>,
    telegram: web::Json<R09ReceiveTelegram>,
    _req: HttpRequest,
) -> impl Responder {
    if app_state.is_poisoned() {
        error!("cannot unwrap app state because the lock is poisenous");
        return web::Json(Response { success: false });
    }

    info!(
        "Received Telegram! {} {:?}",
        &telegram.auth.station, &telegram
    );

    // getting the connection from the postgres connection pool
    let mut database_connection = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!("cannot get connection from connection pool {:?}", e);
            return web::Json(Response { success: false });
        }
    };

    // getting all the meta information from the station and checking
    // if the station is properly authenticated
    let query_response = match authenticate(&mut database_connection, &telegram.auth).await {
        Some(response) => response,
        None => {
            debug!("authentication failed for user {:?}", telegram.auth.station);
            return web::Json(Response { success: false });
        }
    };

    // sends data to the grpc sender
    if query_response.approved {
        match app_state
            .lock()
            .unwrap()
            .grpc_sender
            .lock()
            .unwrap()
            .try_send((
                (*telegram).data.clone(),
                query_response.telegram_meta.clone(),
            )) {
            Err(err) => {
                warn!("[main] Channel GRPC has problems! {:?}", err);
            }
            _ => {}
        }
    }

    // writing telegram into database
    let save_telegram = R09SaveTelegram::from(telegram.data.clone(), query_response.telegram_meta);
    match diesel::insert_into(dump_dvb::schema::r09_telegrams::table)
        .values(&save_telegram)
        .execute(&mut database_connection)
    {
        Err(e) => {
            warn!("Postgres Error {:?} with telegram: {:?}", e, save_telegram);
        }
        _ => {}
    }

    web::Json(Response { success: true })
}

// /telegrams/raw/
pub async fn receiving_raw(
    pool: web::Data<DbPool>,
    _: web::Data<Mutex<ApplicationState>>,
    telegram: web::Json<RawReceiveTelegram>,
    _req: HttpRequest,
) -> impl Responder {
    info!(
        "Received Telegram! {} {:?}",
        &telegram.auth.station, &telegram
    );

    // getting the connection from the postgres connection pool
    let mut database_connection = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!("cannot get connection from connection pool {:?}", e);
            return web::Json(Response { success: false });
        }
    };

    // getting all the meta information from the station and checking
    // if the station is properly authenticated
    let query_response = match authenticate(&mut database_connection, &telegram.auth).await {
        Some(response) => response,
        None => {
            debug!("authentication failed for user {:?}", telegram.auth.station);
            return web::Json(Response { success: false });
        }
    };

    // writing telegram into database
    let save_telegram = RawSaveTelegram::from(telegram.data.clone(), query_response.telegram_meta);
    match diesel::insert_into(dump_dvb::schema::raw_telegrams::table)
        .values(&save_telegram)
        .execute(&mut database_connection)
    {
        Err(e) => {
            warn!("Postgres Error {:?}", e);
        }
        _ => {}
    }

    web::Json(Response { success: true })
}
