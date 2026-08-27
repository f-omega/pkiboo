/// A named value in a compact property list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Property {
    pub label: String,
    pub value: String,
}

impl Property {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// A compact collection of properties describing one object or result.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PropertyList {
    pub title: Option<String>,
    pub properties: Vec<Property>,
}

impl PropertyList {
    pub fn new(properties: impl IntoIterator<Item = Property>) -> Self {
        Self {
            title: None,
            properties: properties.into_iter().collect(),
        }
    }

    pub fn titled(
        title: impl Into<String>,
        properties: impl IntoIterator<Item = Property>,
    ) -> Self {
        Self {
            title: Some(title.into()),
            properties: properties.into_iter().collect(),
        }
    }
}

#[async_trait(?Send)]
pub trait PropertyListView {
    async fn display(&self);
}

/// Structured output that can be presented by either a root UI or a task.
///
/// Domain-specific views should be extension traits that translate their
/// models into these generic property and list models.
pub trait Presenter {
    type List: super::ListView;
    type Properties: PropertyListView;

    fn list<L: super::ListModel + 'static>(&self, list: L) -> Self::List;
    fn property_list(&self, properties: PropertyList) -> Self::Properties;
}
use async_trait::async_trait;
