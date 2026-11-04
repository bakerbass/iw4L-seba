use bevy::prelude::*;

use crate::{ConsoleCommand, ConsoleDispatch};

use super::echo::ConsoleEcho;

pub(crate) fn route_session_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut echo: ConsoleEcho,
    mut transition: ResMut<::session::SessionSwapRequest>,
    has_world: Res<State<frame::MatchScope>>,
    playback: Option<Res<::replay::ReplayPlayback>>,
    bridge: Option<Res<net::MasterBridge>>,
    manifest: Option<Res<::session::SessionContentManifest>>,
    mut dispatch: ResMut<ConsoleDispatch>,
    mut authority: Option<ResMut<net::AuthorityWorld>>,
) {
    for cmd in events.read() {
        match cmd.name.as_str() {
            "round_restart" => {
                let result = if !cmd.args.is_empty() {
                    Err("usage: round_restart".into())
                } else if let Some(authority) = authority.as_deref_mut() {
                    authority.0.request_round_restart()
                } else {
                    Err("round restart needs the match host".into())
                };
                match result {
                    Ok(()) => echo.write("round_restart: requested script round restart"),
                    Err(error) => echo.write(format!("round_restart: {error}")),
                }
            }
            "zom_points" => {
                let amount = match cmd.args.as_slice() {
                    [amount] => amount.parse::<i32>().ok(),
                    _ => None,
                };
                match (amount, authority.as_deref_mut()) {
                    (None, _) => echo.write("usage: zom_points <n>"),
                    (_, None) => echo.write("zom_points: needs the match host"),
                    (_, Some(authority)) if !authority.0.cheats_enabled() => {
                        echo.write("zom_points: cheats are off")
                    }
                    (Some(amount), Some(authority)) => {
                        // Two script profiles: the dm-based mode reads `zom_givepoints`,
                        // BO1's own zombies (`zm`) reads `zm_givepoints`.
                        authority.0.set_gsc_dvar("zom_givepoints", &amount.to_string());
                        authority.0.set_gsc_dvar("zm_givepoints", &amount.to_string());
                        echo.write(format!("zom_points: {amount}"));
                    }
                }
            }
            // The console's `set` writes the menu dvar table; the running script reads its own.
            // Hand `zom_*` values over so scripts can be driven from the console mid-match.
            "set" | "seta" => {
                if let [name, value] = cmd.args.as_slice()
                    && (name.to_ascii_lowercase().starts_with("zom_")
                        || name.to_ascii_lowercase().starts_with("zm_"))
                    && let Some(authority) = authority.as_deref_mut()
                {
                    authority.0.set_gsc_dvar(&name.to_ascii_lowercase(), value);
                }
            }
            "map_restart" => {
                let zone = manifest.as_ref().and_then(|manifest| match &manifest.map {
                    ::session::ManifestFact::Known(map)
                        if (*has_world.get() == frame::MatchScope::Live) =>
                    {
                        Some(format!("{}:{}", map.namespace.as_str(), map.name))
                    }
                    _ => None,
                });
                match (cmd.args.is_empty(), zone) {
                    (false, _) => {
                        dispatch.release();
                        echo.write("usage: map_restart");
                    }
                    (true, None) => {
                        dispatch.release();
                        echo.write("map_restart: no map to restart");
                    }
                    (true, Some(zone)) => match transition.request_zone(zone.clone()) {
                        Ok(id) => {
                            echo.write(format!("map_restart: requested `{zone}` (swap #{id})"))
                        }
                        Err(error) => {
                            dispatch.release();
                            echo.write(format!("map_restart: {error}"));
                        }
                    },
                }
            }
            "map" => match cmd.args.as_slice() {
                [zone] => match transition.request_zone(zone.clone()) {
                    Ok(id) => echo.write(format!("map: requested `{zone}` (swap #{id})")),
                    Err(error) => {
                        dispatch.release();
                        echo.write(format!("map: {error}"));
                    }
                },
                _ => {
                    dispatch.release();
                    echo.write("usage: map <zone>");
                }
            },
            "disconnect" => {
                let in_session = (*has_world.get() == frame::MatchScope::Live)
                    || playback.is_some()
                    || transition.dump_id().is_some()
                    || bridge.is_some();
                if !cmd.args.is_empty() {
                    dispatch.release();
                    echo.write("usage: disconnect");
                } else if !in_session {
                    dispatch.release();
                    echo.write("disconnect: no session to leave");
                } else {
                    match transition.request_leave() {
                        Ok(id) => {
                            diag::lifecycle_boundary(
                                "disconnect_requested",
                                &format!(" swap={id}"),
                            );
                            echo.write(format!(
                                "disconnect: waiting for session teardown (swap #{id})"
                            ))
                        }
                        Err(error) => {
                            dispatch.release();
                            echo.write(format!("disconnect: {error}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
