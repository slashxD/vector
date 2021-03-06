pub mod sink;
pub mod source;
pub mod state;
pub mod transform;

use crate::config::Config;
use async_graphql::{Interface, Object, Subscription};
use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet};
use tokio::stream::{Stream, StreamExt};

#[derive(Debug, Clone, Interface)]
#[graphql(
    field(name = "name", type = "String"),
    field(name = "component_type", type = "String")
)]
pub enum Component {
    Source(source::Source),
    Transform(transform::Transform),
    Sink(sink::Sink),
}

#[derive(Default)]
pub struct ComponentsQuery;

#[Object]
impl ComponentsQuery {
    /// Configured components (sources/transforms/sinks)
    async fn components(&self) -> Vec<Component> {
        state::filter_components(|(_name, components)| Some(components.clone()))
    }

    /// Configured sources
    async fn sources(&self) -> Vec<source::Source> {
        state::get_sources()
    }

    /// Configured transforms
    async fn transforms(&self) -> Vec<transform::Transform> {
        state::get_transforms()
    }

    /// Configured sinks
    async fn sinks(&self) -> Vec<sink::Sink> {
        state::get_sinks()
    }
}

#[derive(Clone, Debug)]
enum ComponentChanged {
    Added(Component),
    Removed(Component),
}

lazy_static! {
    static ref COMPONENT_CHANGED: tokio::sync::broadcast::Sender<ComponentChanged> = {
        let (tx, _) = tokio::sync::broadcast::channel(10);
        tx
    };
}

#[derive(Debug, Default)]
pub struct ComponentsSubscription;

#[Subscription]
impl ComponentsSubscription {
    /// Subscribes to all newly added components
    async fn component_added(&self) -> impl Stream<Item = Component> {
        COMPONENT_CHANGED
            .subscribe()
            .into_stream()
            .filter_map(|c| match c {
                Ok(ComponentChanged::Added(c)) => Some(c),
                _ => None,
            })
    }

    /// Subscribes to all removed components
    async fn component_removed(&self) -> impl Stream<Item = Component> {
        COMPONENT_CHANGED
            .subscribe()
            .into_stream()
            .filter_map(|c| match c {
                Ok(ComponentChanged::Removed(c)) => Some(c),
                _ => None,
            })
    }
}

/// Update the 'global' configuration that will be consumed by component queries
pub fn update_config(config: &Config) {
    let mut new_components = HashMap::new();

    // Sources
    for (name, source) in config.sources.iter() {
        new_components.insert(
            name.to_owned(),
            Component::Source(source::Source(source::Data {
                name: name.to_owned(),
                component_type: source.source_type().to_string(),
                output_type: source.output_type(),
            })),
        );
    }

    // Transforms
    for (name, transform) in config.transforms.iter() {
        new_components.insert(
            name.to_string(),
            Component::Transform(transform::Transform(transform::Data {
                name: name.to_owned(),
                component_type: transform.inner.transform_type().to_string(),
                inputs: transform.inputs.clone(),
            })),
        );
    }

    // Sinks
    for (name, sink) in config.sinks.iter() {
        new_components.insert(
            name.to_string(),
            Component::Sink(sink::Sink(sink::Data {
                name: name.to_owned(),
                component_type: sink.inner.sink_type().to_string(),
                inputs: sink.inputs.clone(),
            })),
        );
    }

    // Get the names of existing components
    let existing_component_names = state::get_component_names();
    let new_component_names = new_components
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<HashSet<String>>();

    // Publish all components that have been removed
    existing_component_names
        .difference(&new_component_names)
        .for_each(|name| {
            let _ =
                COMPONENT_CHANGED.send(ComponentChanged::Removed(state::component_by_name(name)));
        });

    // Publish all components that have been added
    new_component_names
        .difference(&existing_component_names)
        .for_each(|name| {
            let _ = COMPONENT_CHANGED.send(ComponentChanged::Added(
                new_components.get(name).unwrap().clone(),
            ));
        });

    // Override the old component state
    state::update(new_components);
}
