use clap::Parser;
use serde::{Serialize, Deserialize};
use std::str::FromStr;

#[derive(Debug)]
pub struct Name<What> {
    name: String,
    p: std::marker::PhantomData<What>
}

impl<What> Name<What> {
    pub fn new(name: String) -> Self {
        Name { name, p: std::marker::PhantomData{} }
    }
}

impl<What> Clone for Name<What> {
    fn clone(&self) -> Self {
        Self { name: self.name.clone(), p: std::marker::PhantomData {} }
    }
}

impl<What> PartialEq for Name<What> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl<What> PartialOrd for Name<What> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.name.partial_cmp(&other.name)
    }
}

impl<What> std::fmt::Display for Name<What> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl<What> Serialize for Name<What> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        self.name.serialize(serializer)
    }
}

impl<'de, What> Deserialize<'de> for Name<What> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        String::deserialize(deserializer).map(|name| Name { name, p: std::marker::PhantomData{} })
    }
}

impl<'a, What> From<&'a Name<What>> for &'a String {
    fn from(value: &'a Name<What>) -> Self {
        &value.name
    }
}

impl<What> From<Name<What>> for String {
    fn from(value: Name<What>) -> Self {
        value.name
    }
}

impl<What> std::ops::Deref for Name<What> {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.name
    }
}

impl<What> FromStr for Name<What> {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self { name: s.to_owned(), p: std::marker::PhantomData {} })
    }
}

/// Generic arguments for lists
#[derive(clap::Args, Clone)]
pub struct ListOptions {
    /// List of columns to display
    #[arg(long="columns", short='O', value_delimiter=',', value_name="COL1,COL2,...")]
    pub output: Option<Vec<String>>
}

impl ListOptions {
    pub fn new() -> Self {
        ListOptions { output: None }
    }
}
