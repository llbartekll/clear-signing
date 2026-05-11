use std::collections::HashMap;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFile {
    pub descriptor: PathBuf,
    #[serde(default, rename = "dataProvider", skip_serializing_if = "Option::is_none")]
    pub data_provider: Option<DataProviderStub>,
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TestCase {
    Calldata(CalldataCase),
    Eip712(Eip712Case),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalldataCase {
    pub description: String,
    #[serde(rename = "rawTx")]
    pub raw_tx: String,
    #[serde(default, rename = "txHash", skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    pub expected: Expected,
    #[serde(default, rename = "dataProvider", skip_serializing_if = "Option::is_none")]
    pub data_provider: Option<DataProviderStub>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Eip712Case {
    pub description: String,
    pub data: serde_json::Value,
    pub expected: Expected,
    #[serde(default, rename = "dataProvider", skip_serializing_if = "Option::is_none")]
    pub data_provider: Option<DataProviderStub>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expected {
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default)]
    pub fields: IndexMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataProviderStub {
    #[serde(default)]
    pub tokens: HashMap<String, TokenStub>,
    #[serde(default, rename = "addressNames")]
    pub address_names: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStub {
    pub symbol: String,
    pub decimals: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl DataProviderStub {
    pub fn merged(file_level: Option<&Self>, case_level: Option<&Self>) -> Self {
        let mut out = file_level.cloned().unwrap_or_default();
        if let Some(case) = case_level {
            for (k, v) in &case.tokens {
                out.tokens.insert(k.clone(), v.clone());
            }
            for (k, v) in &case.address_names {
                out.address_names.insert(k.clone(), v.clone());
            }
        }
        out
    }
}

impl TestCase {
    pub fn description(&self) -> &str {
        match self {
            TestCase::Calldata(c) => &c.description,
            TestCase::Eip712(c) => &c.description,
        }
    }

    pub fn expected(&self) -> &Expected {
        match self {
            TestCase::Calldata(c) => &c.expected,
            TestCase::Eip712(c) => &c.expected,
        }
    }

    pub fn case_provider(&self) -> Option<&DataProviderStub> {
        match self {
            TestCase::Calldata(c) => c.data_provider.as_ref(),
            TestCase::Eip712(c) => c.data_provider.as_ref(),
        }
    }
}
