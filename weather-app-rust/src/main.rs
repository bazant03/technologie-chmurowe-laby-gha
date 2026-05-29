use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::process::exit;
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server, StatusCode};
use chrono::Local;

#[derive(Clone, Copy)]
struct City {
    name: &'static str,
    lat: f64,
    lon: f64,
}

#[derive(Deserialize)]
struct OpenWeatherResponse {
    weather: Vec<WeatherDescription>,
    main: MainData,
    wind: WindData,
}

#[derive(Deserialize)]
struct WeatherDescription {
    description: String,
}

#[derive(Deserialize)]
struct MainData {
    temp: f64,
    feels_like: f64,
    humidity: i32,
}

#[derive(Deserialize)]
struct WindData {
    speed: f64,
}

struct WeatherData {
    description: String,
    temperature: f64,
    feels_like: f64,
    humidity: i32,
    wind_speed: f64,
}

fn cities() -> HashMap<&'static str, City> {
    let mut m = HashMap::new();
    m.insert(
        "Lublin",
        City {
            name: "Lublin",
            lat: 51.2511,
            lon: 22.575,
        },
    );
    m.insert(
        "Lublin (USA, Wisconsin)",
        City {
            name: "Lublin (USA, Wisconsin)",
            lat: 45.075278,
            lon: -90.724167,
        },
    );
    m.insert(
        "Lubliniec",
        City {
            name: "Lubliniec",
            lat: 50.6688,
            lon: 18.6842,
        },
    );
    m
}

fn main() {
    // Tryb używany przez Docker HEALTHCHECK.
    if env::args().any(|arg| arg == "--healthcheck") {
        run_healthcheck();
        return;
    }

    // Minimalny serwer HTTP nasłuchujący na porcie 80.
    let server = Server::http("0.0.0.0:80").expect("cannot start server on port 80");

    println!("Data uruchomienia: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    println!("Autor: Marek Ruszecki");
    println!("Serwer nasłuchuje na porcie 80");
    for mut request in server.incoming_requests() {
        let url = request.url().to_string();
        let method = request.method().clone();

        // Aplikacja obsługuje tylko GET.
        if method != Method::Get {
            let _ = request.respond(with_html(
                Response::from_string("Method Not Allowed").with_status_code(StatusCode(405)),
            ));
            continue;
        }

        // Strona główna z listą miast.
        if url == "/" {
            let _ = request.respond(with_html(Response::from_string(index_html())));
            continue;
        }

        // Endpoint danych pogodowych dla wybranego miasta.
        if url.starts_with("/weather") {
            let response = match weather_html(&url) {
                Ok(body) => with_html(Response::from_string(body)),
                Err((status, msg)) => with_html(
                    Response::from_string(msg).with_status_code(StatusCode(status)),
                ),
            };
            let _ = request.respond(response);
            continue;
        }

        let _ = request.respond(with_html(
            Response::from_string("Not Found").with_status_code(StatusCode(404)),
        ));
    }
}

fn run_healthcheck() {
    // Krótki ping endpointu aplikacji wewnątrz kontenera.
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("cannot build client");

    match client.get("http://localhost:80/weather?city=Lublin").send() {
        Ok(resp) if resp.status().is_success() => {
            println!("OK {}", resp.status().as_u16());
            exit(0);
        }
        Ok(resp) => {
            println!("ERR {}", resp.status().as_u16());
            exit(1);
        }
        Err(err) => {
            println!("ERR {}", err);
            exit(1);
        }
    }
}

fn with_html(mut response: Response<std::io::Cursor<Vec<u8>>>) -> Response<std::io::Cursor<Vec<u8>>> {
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]) {
        response = response.with_header(header);
    }
    response
}

fn index_html() -> String {
    let city_names = ["Lublin", "Lublin (USA, Wisconsin)", "Lubliniec"];
    let mut options = String::new();
    for city in city_names {
        options.push_str(&format!("<option value=\"{}\">{}</option>", city, city));
    }

    format!(
        "<html><body><h1>Wybierz miasto</h1><form action=\"/weather\" method=\"get\"><select name=\"city\">{}</select><button type=\"submit\">Pokaz pogode</button></form></body></html>",
        options
    )
}

fn weather_html(url: &str) -> Result<String, (u16, String)> {
    // Miasto jest pobierane z query string, np. /weather?city=Lublin.
    let city_name = query_param(url, "city").ok_or((400, "Nieznane miasto".to_string()))?;
    let city_map = cities();
    let city = city_map
        .get(city_name.as_str())
        .ok_or((400, "Nieznane miasto".to_string()))?;

    let data = fetch_weather(*city).map_err(|e| (502, format!("Blad pobierania danych pogodowych: {}", e)))?;

    Ok(format!(
        "<html><body><h1>Pogoda: {}</h1><p>Opis: {}</p><p>Temperatura: {:.1} C</p><p>Odczuwalna: {:.1} C</p><p>Wilgotnosc: {}%</p><p>Wiatr: {:.1} m/s</p><p><a href=\"/\">Powrot</a></p></body></html>",
        city.name,
        data.description,
        data.temperature,
        data.feels_like,
        data.humidity,
        data.wind_speed
    ))
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            // Query z formularza GET koduje spacje jako '+', nie jako '%20'.
            let normalized = v.replace('+', " ");
            if let Ok(decoded) = urlencoding::decode(&normalized) {
                return Some(decoded.into_owned());
            }
        }
    }
    None
}

fn fetch_weather(city: City) -> Result<WeatherData, String> {
    let api_key = env::var("OPENWEATHER_API_KEY").map_err(|_| "brak OPENWEATHER_API_KEY".to_string())?;

    // OpenWeather: lokalizacja po lat/lon, autoryzacja przez appid, jednostki metric, jezyk pl.
    let url = format!(
        "https://api.openweathermap.org/data/2.5/weather?lat={:.4}&lon={:.4}&appid={}&units=metric&lang=pl",
        city.lat, city.lon, api_key
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(url).send().map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("status API: {}", status.as_u16()));
    }

    let parsed: OpenWeatherResponse = response.json().map_err(|e| e.to_string())?;
    // Pobieramy pierwszy opis pogody z tablicy weather.
    let description = parsed
        .weather
        .first()
        .map(|w| w.description.clone())
        .ok_or("brak pola weather w odpowiedzi".to_string())?;

    Ok(WeatherData {
        description,
        temperature: parsed.main.temp,
        feels_like: parsed.main.feels_like,
        humidity: parsed.main.humidity,
        wind_speed: parsed.wind.speed,
    })
}
