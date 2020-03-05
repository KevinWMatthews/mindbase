use crate::{
    agent::AgentId,
    artifact::ArtifactId,
    Agent,
};

use rusty_ulid::generate_ulid_bytes;
use serde::{
    Deserialize,
    Serialize,
};
use std::fmt;
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct AllegationId(#[serde(serialize_with = "crate::util::serde_helper::as_base64",
                                deserialize_with = "crate::util::serde_helper::from_base64")]
                        pub(crate) [u8; 16]);

impl AllegationId {
    pub fn new(agent: &Agent, body: Body) -> Self {
        AllegationId(generate_ulid_bytes())
    }

    pub fn base64(&self) -> String {
        use base64::STANDARD_NO_PAD;
        base64::encode_config(&self.0, STANDARD_NO_PAD)
    }
}

impl std::convert::AsRef<[u8]> for AllegationId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for AllegationId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use base64::STANDARD_NO_PAD;
        write!(f, "{}", base64::encode_config(&self.0, STANDARD_NO_PAD))
    }
}
impl fmt::Debug for AllegationId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EntityID:{}", base64::encode(&self.0))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Allegation {
    pub id:   AllegationId,
    pub by:   AgentId,
    pub body: Body,

    #[serde(serialize_with = "crate::util::array64::ser_as_base64",
            deserialize_with = "crate::util::array64::de_from_base64")]
    pub signature: [u8; 64],
}

impl Allegation {
    /// Create a concept which points exclusively to this allegation
    /// Narrow concepts should be created ONLY when referring to some other entities we just created
    /// Otherwise it is lazy, and will result in a non-convergent graph
    pub fn narrow_concept(&self) -> Concept {
        Concept { members:       vec![self.id().clone()],
                  spread_factor: 0.0, }
    }

    pub fn id(&self) -> &AllegationId {
        &self.id
    }
}

impl fmt::Display for Allegation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.id, self.body)
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum Body {
    Unit,
    Agent(AgentId),
    Analogy(Analogy),
    Artifact(ArtifactId),
}

impl fmt::Display for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Body::Unit => write!(f, "Unit()"),
            Body::Agent(a) => write!(f, "Agent({})", a),
            Body::Allegation(a) => write!(f, "Allegation({})", a),
            Body::Artifact(a) => write!(f, "Artifact({})", a),
        }
    }
}
