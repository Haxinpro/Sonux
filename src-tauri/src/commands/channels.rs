use tauri::State;

use crate::audio::types::VirtualSink;
use crate::persistence::wireplumber;
use crate::state::AppState;

#[derive(Clone)]
struct PersistedChannelState {
    defs: crate::persistence::channels::Channels,
    assignments: crate::persistence::assignments::Assignments,
    outputs: crate::persistence::outputs::ChannelOutputs,
    eq: crate::persistence::eq::ChannelEq,
    buses: crate::persistence::buses::Buses,
}

impl PersistedChannelState {
    fn save(&self) -> Result<(), crate::error::SinkError> {
        self.defs.save()?;
        self.assignments.save()?;
        self.outputs.save()?;
        self.eq.save()?;
        self.buses.save()?;
        wireplumber::write(&self.assignments)
    }

    fn restore(&self) -> Result<(), crate::error::SinkError> {
        let mut errors = Vec::new();
        macro_rules! attempt {
            ($label:literal, $result:expr) => {
                if let Err(error) = $result {
                    errors.push(format!(concat!($label, ": {}"), error));
                }
            };
        }
        attempt!("channel definitions", self.defs.save());
        attempt!("assignments", self.assignments.save());
        attempt!("outputs", self.outputs.save());
        attempt!("equalizer settings", self.eq.save());
        attempt!("mix definitions", self.buses.save());
        attempt!("WirePlumber rules", wireplumber::write(&self.assignments));
        if errors.is_empty() {
            Ok(())
        } else {
            Err(crate::error::SinkError::Config(errors.join("; ")))
        }
    }
}

fn persistence_error_with_restore(
    error: impl std::fmt::Display,
    previous: &PersistedChannelState,
) -> String {
    match previous.restore() {
        Ok(()) => error.to_string(),
        Err(rollback_error) => {
            format!("{error}; restoring the previous channel files also failed: {rollback_error}")
        }
    }
}

fn restore_channel_runtime(
    state: &AppState,
    channel: &VirtualSink,
    output: Option<&str>,
    failover: bool,
    eq: &crate::audio::types::EqConfig,
    streams: &[u32],
) -> Result<(), crate::error::SinkError> {
    state
        .backend
        .set_sink_volume(&channel.name, channel.volume_percent)?;
    state.backend.set_sink_mute(&channel.name, channel.muted)?;
    state.backend.set_channel_output(&channel.name, output)?;
    state
        .backend
        .set_channel_failover(&channel.name, failover)?;
    if state.backend_native {
        state.backend.set_channel_eq(&channel.name, eq)?;
    }
    for index in streams {
        state.backend.move_stream_to_sink(*index, &channel.name)?;
    }
    Ok(())
}

fn add_channel_failure(
    error: impl std::fmt::Display,
    rollbacks: &[(&str, Result<(), crate::error::SinkError>)],
) -> String {
    let mut message = error.to_string();
    for (action, rollback) in rollbacks {
        if let Err(rollback_error) = rollback {
            message.push_str(&format!("; {action} also failed: {rollback_error}"));
        }
    }
    message
}

fn rollback_added_channel(
    state: &AppState,
    sink_name: &str,
    buses: &crate::persistence::buses::Buses,
    old_names: &[String],
    applied_buses: &[String],
) -> Result<(), crate::error::SinkError> {
    let mut errors = Vec::new();
    for name in applied_buses.iter().rev() {
        if let Some(bus) = buses.get(name) {
            if let Err(error) = state
                .backend
                .set_bus_members(name, &bus.effective_members(old_names))
            {
                errors.push(format!("restore members for {name}: {error}"));
            }
        }
    }
    if let Err(error) = state.backend.destroy_virtual_sink(sink_name) {
        errors.push(format!("remove new channel: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(crate::error::SinkError::Config(errors.join("; ")))
    }
}

fn restore_bus_memberships(
    state: &AppState,
    buses: &crate::persistence::buses::Buses,
    channel_names: &[String],
    applied: &[String],
) -> Result<(), crate::error::SinkError> {
    let mut errors = Vec::new();
    for name in applied.iter().rev() {
        if let Some(bus) = buses.get(name) {
            if let Err(error) = state
                .backend
                .set_bus_members(name, &bus.effective_members(channel_names))
            {
                errors.push(format!("restore members for {name}: {error}"));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(crate::error::SinkError::Config(errors.join("; ")))
    }
}

fn restore_stream_routes(
    state: &AppState,
    stream_indices: &[u32],
    sink_name: &str,
) -> Result<(), crate::error::SinkError> {
    let errors = stream_indices
        .iter()
        .filter_map(|index| {
            state
                .backend
                .move_stream_to_sink(*index, sink_name)
                .err()
                .map(|error| format!("stream {index}: {error}"))
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(crate::error::SinkError::Config(errors.join("; ")))
    }
}

struct ChannelEdit<'a> {
    previous_defs: &'a crate::persistence::channels::Channels,
    defs: &'a crate::persistence::channels::Channels,
    previous_channels: &'a [VirtualSink],
    channels: &'a [VirtualSink],
    buses: &'a crate::persistence::buses::Buses,
}

fn persist_channel_edit(
    mixer: &crate::mixer::state::MixerState,
    edit: ChannelEdit<'_>,
    rollback_action: &str,
    rollback_live: impl Fn() -> Result<(), crate::error::SinkError>,
) -> Result<(), String> {
    if let Err(error) = edit.defs.save() {
        return Err(add_channel_failure(
            error,
            &[
                (
                    "restoring the previous channel definitions",
                    edit.previous_defs.save(),
                ),
                (rollback_action, rollback_live()),
            ],
        ));
    }
    if let Err(error) = crate::commands::profiles::save_active_with_channels_and_buses(
        mixer,
        edit.channels,
        edit.buses,
    ) {
        return Err(add_channel_failure(
            error,
            &[
                (
                    "restoring the previous active profile",
                    crate::commands::profiles::save_active_with_channels_and_buses(
                        mixer,
                        edit.previous_channels,
                        edit.buses,
                    ),
                ),
                (
                    "restoring the previous channel definitions",
                    edit.previous_defs.save(),
                ),
                (rollback_action, rollback_live()),
            ],
        ));
    }
    Ok(())
}

/// Create a new channel from a label and icon (sink name is generated).
/// The new channel starts at 100%, unmuted, following the default output.
#[tauri::command]
pub fn add_channel(
    state: State<'_, AppState>,
    label: String,
    icon: Option<String>,
    spatial: bool,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (def, previous, next, next_channels, old_names, new_names, prefs) = {
        let mixer = state.lock_mixer()?;
        let previous = PersistedChannelState {
            defs: mixer.channel_defs.clone(),
            assignments: mixer.assignments.clone(),
            outputs: mixer.outputs.clone(),
            eq: mixer.eq.clone(),
            buses: mixer.buses.clone(),
        };
        let mut next = previous.clone();
        let def = next
            .defs
            .add_with_spatial(&label, icon, spatial)
            .map_err(|e| e.to_string())?;
        let mut next_channels = mixer.channels.clone();
        next_channels.push(VirtualSink {
            name: def.name.clone(),
            label: def.label.clone(),
            icon: def.icon.clone(),
            volume_percent: 100,
            muted: false,
            stream_mix: def.stream_mix,
        });
        let old_names = mixer
            .channels
            .iter()
            .map(|channel| channel.name.clone())
            .collect::<Vec<_>>();
        let new_names = next_channels
            .iter()
            .map(|channel| channel.name.clone())
            .collect::<Vec<_>>();
        next.buses.sync_master(&new_names);
        (
            def,
            previous,
            next,
            next_channels,
            old_names,
            new_names,
            mixer.prefs.clone(),
        )
    };

    if let Err(e) = (|| {
        state
            .backend
            .create_virtual_sink(&def.name, &prefs.decorate(&def.label))?;
        state.backend.set_sink_volume(&def.name, 100)?;
        state.backend.set_sink_mute(&def.name, false)?;
        state.backend.set_channel_output(&def.name, None)
    })() {
        // The candidate definition has not reached memory or disk yet.
        let _ = state.backend.destroy_virtual_sink(&def.name);
        return Err(e.to_string());
    }

    let mut applied_buses = Vec::new();
    if state.backend_native {
        for bus in &next.buses.buses {
            if let Err(error) = state
                .backend
                .set_bus_members(&bus.name, &bus.effective_members(&new_names))
            {
                return Err(add_channel_failure(
                    error,
                    &[(
                        "restoring the previous live channel graph",
                        rollback_added_channel(
                            &state,
                            &def.name,
                            &previous.buses,
                            &old_names,
                            &applied_buses,
                        ),
                    )],
                ));
            }
            applied_buses.push(bus.name.clone());
        }
    }
    if let Err(error) = next.save() {
        return Err(add_channel_failure(
            error,
            &[
                ("restoring the previous channel files", previous.restore()),
                (
                    "restoring the previous live channel graph",
                    rollback_added_channel(
                        &state,
                        &def.name,
                        &previous.buses,
                        &old_names,
                        &applied_buses,
                    ),
                ),
            ],
        ));
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_channels_and_buses(
            &mixer,
            &next_channels,
            &next.buses,
        ) {
            return Err(add_channel_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_channels_and_buses(
                            &mixer,
                            &mixer.channels,
                            &previous.buses,
                        ),
                    ),
                    ("restoring the previous channel files", previous.restore()),
                    (
                        "restoring the previous live channel graph",
                        rollback_added_channel(
                            &state,
                            &def.name,
                            &previous.buses,
                            &old_names,
                            &applied_buses,
                        ),
                    ),
                ],
            ));
        }
    }
    let mut mixer = state.lock_mixer()?;
    mixer.channel_defs = next.defs;
    mixer.channels = next_channels;
    mixer.buses = next.buses;
    Ok(())
}

/// Reorder the channel strips (cosmetic - no audio plumbing changes).
#[tauri::command]
pub fn reorder_channels(
    state: State<'_, AppState>,
    order: Vec<String>,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (previous_defs, defs, previous_channels, channels, buses) = {
        let mixer = state.lock_mixer()?;
        let previous_defs = mixer.channel_defs.clone();
        let mut defs = previous_defs.clone();
        defs.reorder(&order).map_err(|e| e.to_string())?;
        let previous_channels = mixer.channels.clone();
        let mut channels = previous_channels.clone();
        channels.sort_by_key(|c| {
            order
                .iter()
                .position(|n| n == &c.name)
                .unwrap_or(usize::MAX)
        });
        (
            previous_defs,
            defs,
            previous_channels,
            channels,
            mixer.buses.clone(),
        )
    };
    {
        let mixer = state.lock_mixer()?;
        persist_channel_edit(
            &mixer,
            ChannelEdit {
                previous_defs: &previous_defs,
                defs: &defs,
                previous_channels: &previous_channels,
                channels: &channels,
                buses: &buses,
            },
            "restoring the live channel order",
            || Ok(()),
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.channel_defs = defs;
    mixer.channels = channels;
    Ok(())
}

/// Change a channel's strip icon.
#[tauri::command]
pub fn set_channel_icon(
    state: State<'_, AppState>,
    sink_name: String,
    icon: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let icon = if icon.is_empty() { None } else { Some(icon) };
    let (previous_defs, defs, previous_channels, channels, buses) = {
        let mixer = state.lock_mixer()?;
        let previous_defs = mixer.channel_defs.clone();
        let mut defs = previous_defs.clone();
        defs.set_icon(&sink_name, icon.clone())
            .map_err(|e| e.to_string())?;
        let previous_channels = mixer.channels.clone();
        let mut channels = previous_channels.clone();
        if let Some(channel) = channels
            .iter_mut()
            .find(|channel| channel.name == sink_name)
        {
            channel.icon = icon;
        }
        (
            previous_defs,
            defs,
            previous_channels,
            channels,
            mixer.buses.clone(),
        )
    };
    {
        let mixer = state.lock_mixer()?;
        persist_channel_edit(
            &mixer,
            ChannelEdit {
                previous_defs: &previous_defs,
                defs: &defs,
                previous_channels: &previous_channels,
                channels: &channels,
                buses: &buses,
            },
            "restoring the live channel icon",
            || Ok(()),
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.channel_defs = defs;
    mixer.channels = channels;
    Ok(())
}

/// Rename a channel's display label (the sink name stays stable, so
/// assignments, outputs and profiles keep working).
#[tauri::command]
pub fn rename_channel(
    state: State<'_, AppState>,
    sink_name: String,
    label: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    let (
        old_channel,
        new_label,
        previous_defs,
        defs,
        previous_channels,
        channels,
        buses,
        prefs,
        output,
        failover,
        eq,
    ) = {
        let mixer = state.lock_mixer()?;
        let old_channel = mixer
            .channels
            .iter()
            .find(|channel| channel.name == sink_name)
            .cloned()
            .ok_or_else(|| format!("unknown channel: {sink_name}"))?;
        let previous_defs = mixer.channel_defs.clone();
        let mut defs = previous_defs.clone();
        defs.rename(&sink_name, &label).map_err(|e| e.to_string())?;
        let new_label = defs
            .get(&sink_name)
            .map(|definition| definition.label.clone())
            .ok_or_else(|| format!("unknown channel: {sink_name}"))?;
        let previous_channels = mixer.channels.clone();
        let mut channels = previous_channels.clone();
        if let Some(channel) = channels
            .iter_mut()
            .find(|channel| channel.name == sink_name)
        {
            channel.label = new_label.clone();
        }
        (
            old_channel,
            new_label,
            previous_defs,
            defs,
            previous_channels,
            channels,
            mixer.buses.clone(),
            mixer.prefs.clone(),
            mixer.outputs.get(&sink_name).map(str::to_string),
            mixer.outputs.failover(&sink_name),
            mixer.eq.get(&sink_name),
        )
    };
    let streams = state
        .backend
        .list_app_streams()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|stream| stream.assigned_sink.as_deref() == Some(sink_name.as_str()))
        .map(|stream| stream.index)
        .collect::<Vec<_>>();
    let decorated_old = prefs.decorate(&old_channel.label);
    let decorated_new = prefs.decorate(&new_label);
    let apply_result = state
        .backend
        .set_virtual_sink_label(&sink_name, &decorated_new)
        .and_then(|()| {
            restore_channel_runtime(
                &state,
                &old_channel,
                output.as_deref(),
                failover,
                &eq,
                &streams,
            )
        });
    if let Err(error) = apply_result {
        let rollback = state
            .backend
            .set_virtual_sink_label(&sink_name, &decorated_old)
            .and_then(|()| {
                restore_channel_runtime(
                    &state,
                    &old_channel,
                    output.as_deref(),
                    failover,
                    &eq,
                    &streams,
                )
            });
        return Err(match rollback {
            Ok(()) => error.to_string(),
            Err(rollback_error) => {
                format!("{error}; restoring the previous channel also failed: {rollback_error}")
            }
        });
    }
    {
        let mixer = state.lock_mixer()?;
        persist_channel_edit(
            &mixer,
            ChannelEdit {
                previous_defs: &previous_defs,
                defs: &defs,
                previous_channels: &previous_channels,
                channels: &channels,
                buses: &buses,
            },
            "restoring the previous live channel",
            || {
                state
                    .backend
                    .set_virtual_sink_label(&sink_name, &decorated_old)
                    .and_then(|()| {
                        restore_channel_runtime(
                            &state,
                            &old_channel,
                            output.as_deref(),
                            failover,
                            &eq,
                            &streams,
                        )
                    })
            },
        )?;
    }
    let mut mixer = state.lock_mixer()?;
    mixer.channel_defs = defs;
    mixer.channels = channels;
    Ok(())
}

/// Delete a channel: streams on it return to the default sink, its
/// assignments are dropped, and the sink is destroyed.
#[tauri::command]
pub fn remove_channel(
    state: State<'_, AppState>,
    sink_name: String,
    expected_profile: Option<String>,
) -> Result<(), String> {
    let _profile_operation = state.lock_expected_profile_operation(expected_profile.as_deref())?;
    // Build and persist a complete candidate before mutating PipeWire or
    // memory. If one of the multi-file writes fails, rewrite the previous
    // snapshot so a partial candidate cannot survive.
    let (previous, next, next_channels, old_names, names) = {
        let mixer = state.lock_mixer()?;
        let previous = PersistedChannelState {
            defs: mixer.channel_defs.clone(),
            assignments: mixer.assignments.clone(),
            outputs: mixer.outputs.clone(),
            eq: mixer.eq.clone(),
            buses: mixer.buses.clone(),
        };
        let mut next = previous.clone();
        next.defs.remove(&sink_name).map_err(|e| e.to_string())?;
        next.assignments
            .assignments
            .retain(|assignment| assignment.sink_name != sink_name);
        next.outputs.remove(&sink_name);
        next.eq.remove(&sink_name);
        next.buses.remove_channel(&sink_name);
        let next_channels = mixer
            .channels
            .iter()
            .filter(|channel| channel.name != sink_name)
            .cloned()
            .collect::<Vec<_>>();
        let names = next_channels
            .iter()
            .map(|channel| channel.name.clone())
            .collect::<Vec<_>>();
        let old_names = mixer
            .channels
            .iter()
            .map(|channel| channel.name.clone())
            .collect::<Vec<_>>();
        (previous, next, next_channels, old_names, names)
    };
    if let Err(error) = next.save() {
        return Err(persistence_error_with_restore(error, &previous));
    }
    {
        let mixer = state.lock_mixer()?;
        if let Err(error) = crate::commands::profiles::save_active_with_channel_state(
            &mixer,
            &next_channels,
            &next.assignments,
            &next.outputs,
            &next.eq,
            &next.buses,
        ) {
            return Err(add_channel_failure(
                error,
                &[
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_channel_state(
                            &mixer,
                            &mixer.channels,
                            &previous.assignments,
                            &previous.outputs,
                            &previous.eq,
                            &previous.buses,
                        ),
                    ),
                    ("restoring the previous channel files", previous.restore()),
                ],
            ));
        }
    }

    // Reconcile every live mix while the old channel still exists, so a
    // failure can be compensated without recreating a deleted PipeWire node.
    let mut applied_buses = Vec::new();
    if state.backend_native {
        for bus in &next.buses.buses {
            if let Err(error) = state
                .backend
                .set_bus_members(&bus.name, &bus.effective_members(&names))
            {
                let mixer = state.lock_mixer()?;
                return Err(add_channel_failure(
                    error,
                    &[
                        (
                            "restoring previous live mix memberships",
                            restore_bus_memberships(
                                &state,
                                &previous.buses,
                                &old_names,
                                &applied_buses,
                            ),
                        ),
                        (
                            "restoring the previous active profile",
                            crate::commands::profiles::save_active_with_channel_state(
                                &mixer,
                                &mixer.channels,
                                &previous.assignments,
                                &previous.outputs,
                                &previous.eq,
                                &previous.buses,
                            ),
                        ),
                        ("restoring the previous channel files", previous.restore()),
                    ],
                ));
            }
            applied_buses.push(bus.name.clone());
        }
    }

    // Hand the channel's streams back to the default sink before the rug
    // is pulled out from under them. Remember successful moves so a failed
    // destruction can restore the graph exactly.
    let mut evacuated = Vec::new();
    let streams = match state.backend.list_app_streams() {
        Ok(streams) => streams,
        Err(error) => {
            let mixer = state.lock_mixer()?;
            return Err(add_channel_failure(
                format!("list streams before removing {sink_name}: {error}"),
                &[
                    (
                        "restoring previous live mix memberships",
                        restore_bus_memberships(
                            &state,
                            &previous.buses,
                            &old_names,
                            &applied_buses,
                        ),
                    ),
                    (
                        "restoring the previous active profile",
                        crate::commands::profiles::save_active_with_channel_state(
                            &mixer,
                            &mixer.channels,
                            &previous.assignments,
                            &previous.outputs,
                            &previous.eq,
                            &previous.buses,
                        ),
                    ),
                    ("restoring the previous channel files", previous.restore()),
                ],
            ));
        }
    };
    for stream in streams {
        if stream.assigned_sink.as_deref() == Some(sink_name.as_str()) {
            match state.backend.move_stream_to_sink(stream.index, "") {
                Ok(()) => evacuated.push(stream.index),
                Err(error) => {
                    let restore_streams = restore_stream_routes(&state, &evacuated, &sink_name);
                    let mixer = state.lock_mixer()?;
                    return Err(add_channel_failure(
                        format!("evacuating {} failed: {error}", stream.app_name),
                        &[
                            ("restoring evacuated streams", restore_streams),
                            (
                                "restoring previous live mix memberships",
                                restore_bus_memberships(
                                    &state,
                                    &previous.buses,
                                    &old_names,
                                    &applied_buses,
                                ),
                            ),
                            (
                                "restoring the previous active profile",
                                crate::commands::profiles::save_active_with_channel_state(
                                    &mixer,
                                    &mixer.channels,
                                    &previous.assignments,
                                    &previous.outputs,
                                    &previous.eq,
                                    &previous.buses,
                                ),
                            ),
                            ("restoring the previous channel files", previous.restore()),
                        ],
                    ));
                }
            }
        }
    }

    if let Err(error) = state.backend.destroy_virtual_sink(&sink_name) {
        let restore_streams = restore_stream_routes(&state, &evacuated, &sink_name);
        let mixer = state.lock_mixer()?;
        return Err(add_channel_failure(
            error,
            &[
                ("restoring evacuated streams", restore_streams),
                (
                    "restoring previous live mix memberships",
                    restore_bus_memberships(&state, &previous.buses, &old_names, &applied_buses),
                ),
                (
                    "restoring the previous active profile",
                    crate::commands::profiles::save_active_with_channel_state(
                        &mixer,
                        &mixer.channels,
                        &previous.assignments,
                        &previous.outputs,
                        &previous.eq,
                        &previous.buses,
                    ),
                ),
                ("restoring the previous channel files", previous.restore()),
            ],
        ));
    }

    {
        let mut mixer = state.lock_mixer()?;
        mixer.channel_defs = next.defs;
        mixer.channels = next_channels;
        mixer.assignments = next.assignments;
        mixer.outputs = next.outputs;
        mixer.eq = next.eq;
        mixer.buses = next.buses;
        // Re-evaluate auto-routing with the channel gone.
        mixer.auto_routed.clear();
    }

    Ok(())
}
