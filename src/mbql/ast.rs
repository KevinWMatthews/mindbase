pub mod artifact;

use crate::{
    mbql::{
        error::MBQLError,
        parse::{
            self,
            Rule,
        },
        Position,
        Query,
    },
    AgentId,
    ArtifactId,
    Concept,
    GroundSymbolize,
    MBError,
    MindBase,
};

use pest::iterators::Pair;

// trait ParseWrite {
//     fn parse(pair: Pair<parse::Rule>) -> Self;
//     fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), std::io::Error>;
// }

#[derive(Debug)]
pub struct ArtifactVar {
    pub var:      String,
    pub position: Position,
}

impl ArtifactVar {
    fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::artifactvar);
        Ok(Self { var: pair.into_inner().next().unwrap().as_str().to_string(),
                  position })
    }

    fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), std::io::Error> {
        writer.write(format!("@{}", self.var).as_bytes())?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct SymbolVar {
    pub var:      String,
    pub position: Position,
}

impl SymbolVar {
    pub fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::symbolvar);
        Ok(Self { var: pair.into_inner().next().unwrap().as_str().to_string(),
                  position })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), std::io::Error> {
        writer.write(format!("${}", self.var).as_bytes())?;
        Ok(())
    }

    pub fn to_string(&self) -> String {
        self.var.clone()
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub struct ArtifactStatement {
    pub var:      ArtifactVar,
    pub artifact: Artifact,
    pub position: Position,
}

impl ArtifactStatement {
    pub fn parse(pair: Pair<Rule>, position: Position, query: &mut crate::mbql::Query) -> Result<(), MBQLError> {
        assert_eq!(pair.as_rule(), Rule::artifactstatement);

        let mut pairs = pair.into_inner();
        let var = ArtifactVar::parse(pairs.next().unwrap(), position.clone())?;

        let artifact = Artifact::parse(pairs.next().unwrap(), position.clone())?;

        let me = ArtifactStatement { var, artifact, position };

        query.add_artifact_statement(me);

        Ok(())
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), std::io::Error> {
        self.var.write(writer)?;
        writer.write(b" = ")?;
        self.artifact.write(writer, true)?;
        writer.write(b"\n")?;
        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<ArtifactId, MBQLError> {
        let artifact_id = self.artifact.apply(query, mb)?;
        query.store_artifact_for_var(&self.var, artifact_id.clone())?;
        Ok(artifact_id)
    }
}

#[derive(Debug)]
pub struct SymbolStatement {
    pub var:    Option<SymbolVar>,
    pub symbol: Symbolizable,
}

impl SymbolStatement {
    pub fn parse(pair: Pair<Rule>, query: &mut crate::mbql::Query, position: Position) -> Result<(), MBQLError> {
        assert_eq!(pair.as_rule(), Rule::symbolstatement);

        let mut pairs = pair.into_inner();

        let next = pairs.next().unwrap();

        let (var, next) = if let Rule::symbolvar = next.as_rule() {
            (Some(SymbolVar::parse(next, position.clone())?), pairs.next().unwrap())
        } else {
            (None, next)
        };

        // based on the grammar, we are guaranteed to have allege | ground | symbolize
        let symbol = match next.as_rule() {
            Rule::allege => Symbolizable::Allege(Allege::parse(next, position.clone())?),
            Rule::ground => Symbolizable::Ground(Ground::parse(next, position.clone())?),
            Rule::symbolize => Symbolizable::Symbolize(Symbolize::parse(next, position.clone())?),
            _ => unreachable!(),
        };

        let me = SymbolStatement { var, symbol };

        query.add_symbol_statement(me);

        Ok(())
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), std::io::Error> {
        if let Some(var) = &self.var {
            var.write(writer)?;
            writer.write(b" = ")?;
        }
        self.symbol.write(writer, true, false)?;
        writer.write(b"\n")?;
        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        self.symbol.apply(query, mb)
    }
}

#[derive(Debug)]
pub struct Ground {
    symbolizable: Box<GroundSymbolizable>,
    position:     Position,
}

impl Ground {
    fn parse(pair: Pair<Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::ground);

        Ok(Ground { symbolizable: Box::new(GroundSymbolizable::parse(pair.into_inner().next().unwrap(), position.clone())?),
                    position })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, verbose: bool) -> Result<(), std::io::Error> {
        if verbose {
            writer.write(b"Ground(")?;
            self.symbolizable.write(writer, false, false)?;
            writer.write(b")")?;
        } else {
            writer.write(b"{")?;
            self.symbolizable.write(writer, false, false)?;
            writer.write(b"}")?;
        }
        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        self.symbolizable.apply(query, mb)
    }
}

#[derive(Debug)]
pub struct Allege {
    left:  Box<Symbolizable>,
    right: Box<Symbolizable>,
}

impl Allege {
    pub fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::allege);

        let mut symbol_pair = pair.into_inner().next().unwrap().into_inner();

        // According to the grammar, Allege may only contain symbol_pair
        let left = Symbolizable::parse(symbol_pair.next().unwrap(), position.clone())?;
        let right = Symbolizable::parse(symbol_pair.next().unwrap(), position.clone())?;

        Ok(Allege { left:  Box::new(left),
                    right: Box::new(right), })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, verbose: bool, nest: bool) -> Result<(), std::io::Error> {
        if verbose {
            writer.write(b"Allege(")?;
        } else if nest {
            writer.write(b"(")?;
        }

        self.left.write(writer, false, true)?;
        writer.write(b" : ")?;
        self.right.write(writer, false, true)?;

        if verbose {
            writer.write(b")")?;
        } else if nest {
            writer.write(b")")?;
        }
        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub struct Symbolize(Box<Symbolizable>);
impl Symbolize {
    pub fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::symbolize);
        Ok(Symbolize(Box::new(Symbolizable::parse(pair.into_inner().next().unwrap(), position)?)))
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, verbose: bool) -> Result<(), std::io::Error> {
        if verbose {
            writer.write(b"Symbolize(")?;
        }

        self.0.write(writer, false, false)?;

        if verbose {
            writer.write(b")")?;
        }
        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub enum Symbolizable {
    Artifact(Artifact),
    Allege(Allege),
    // TODO 1 - determine if we want to flatten/variablize/pointerize the tree as we parse it
    // or if we flatten that structure at a later phase?
    SymbolVar(SymbolVar),
    Ground(Ground),
    Symbolize(Symbolize),
}

impl Symbolizable {
    pub fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        // because of left-recursion issues, we had to construct symbolizable in a slightly odd way
        // which necessitates allege and ground to support symbol_pair AND symbolizable as potential child elements
        // So we are handling symbol_pair if they were symbolizable
        let s = match pair.as_rule() {
            Rule::symbol_pair => {
                let mut inner = pair.into_inner();
                let left = Symbolizable::parse(inner.next().unwrap(), position.clone())?;
                let right = Symbolizable::parse(inner.next().unwrap(), position.clone())?;
                Symbolizable::Allege(Allege { left:  Box::new(left),
                                              right: Box::new(right), })
            },
            Rule::symbolizable => {
                let element = pair.into_inner().next().unwrap();

                match element.as_rule() {
                    Rule::artifact => Symbolizable::Artifact(Artifact::parse(element, position)?),
                    Rule::symbolvar => Symbolizable::SymbolVar(SymbolVar::parse(element, position)?),
                    Rule::ground => Symbolizable::Ground(Ground::parse(element, position)?),
                    Rule::symbolize => Symbolizable::Symbolize(Symbolize::parse(element, position)?),
                    Rule::allege => Symbolizable::Allege(Allege::parse(element, position)?),
                    Rule::symbol_pair => {
                        let mut inner = element.into_inner();
                        let left = Symbolizable::parse(inner.next().unwrap(), position.clone())?;
                        let right = Symbolizable::parse(inner.next().unwrap(), position.clone())?;
                        Symbolizable::Allege(Allege { left:  Box::new(left),
                                                      right: Box::new(right), })
                    },
                    _ => unreachable!(),
                }
            },
            _ => unreachable!(),
        };

        Ok(s)
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, verbose: bool, nest: bool) -> Result<(), std::io::Error> {
        match self {
            Symbolizable::Artifact(a) => a.write(writer, verbose)?,
            Symbolizable::Allege(a) => a.write(writer, verbose, nest)?,
            Symbolizable::SymbolVar(sv) => sv.write(writer)?,
            Symbolizable::Ground(g) => g.write(writer, verbose)?,
            Symbolizable::Symbolize(s) => s.write(writer, verbose)?,
        }

        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        let symbol = match self {
            Symbolizable::Artifact(a) => {
                let artifact_id = a.apply(query, mb)?;
                mb.symbolize(artifact_id)?
            },
            // Symbolizable::Allege(a) => a.apply(query, mb),
            // Symbolizable::SymbolVar(sv) => sv.apply(query, mb),
            Symbolizable::Ground(g) => g.apply(query, mb)?,
            // Symbolizable::Symbolize(s) => s.apply(query, mb),
            _ => unimplemented!(),
        };

        Ok(symbol)
    }
}

#[derive(Debug)]
pub enum GroundSymbolizable {
    Artifact(Artifact),
    SymbolVar(SymbolVar),
    Ground(Ground),
    GroundPair(GroundPair),
}

impl GroundSymbolizable {
    pub fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        let s = match pair.as_rule() {
            Rule::ground_symbol_pair => {
                let mut inner = pair.into_inner();
                let left = GroundSymbolizable::parse(inner.next().unwrap(), position.clone())?;
                let right = GroundSymbolizable::parse(inner.next().unwrap(), position.clone())?;
                GroundSymbolizable::GroundPair(GroundPair { left: Box::new(left),
                                                            right: Box::new(right),
                                                            position })
            },
            Rule::ground_symbolizable => {
                let element = pair.into_inner().next().unwrap();

                match element.as_rule() {
                    Rule::artifact => GroundSymbolizable::Artifact(Artifact::parse(element, position)?),
                    Rule::symbolvar => GroundSymbolizable::SymbolVar(SymbolVar::parse(element, position)?),
                    Rule::ground => GroundSymbolizable::Ground(Ground::parse(element, position)?),
                    Rule::ground_symbol_pair => {
                        let mut inner = element.into_inner();
                        let left = GroundSymbolizable::parse(inner.next().unwrap(), position.clone())?;
                        let right = GroundSymbolizable::parse(inner.next().unwrap(), position.clone())?;
                        GroundSymbolizable::GroundPair(GroundPair { left: Box::new(left),
                                                                    right: Box::new(right),
                                                                    position })
                    },
                    _ => unreachable!(),
                }
            },
            _ => unreachable!(),
        };

        Ok(s)
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, verbose: bool, nest: bool) -> Result<(), std::io::Error> {
        match self {
            GroundSymbolizable::Artifact(a) => a.write(writer, verbose)?,
            GroundSymbolizable::GroundPair(p) => p.write(writer, nest)?,
            GroundSymbolizable::SymbolVar(sv) => sv.write(writer)?,
            GroundSymbolizable::Ground(g) => g.write(writer, verbose)?,
        }

        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        let symbol = match self {
            GroundSymbolizable::Artifact(a) => {
                let artifact_id = a.apply(query, mb)?;
                mb.get_ground_symbol(artifact_id)?
            },
            //     GroundSymbolizable::GroundPair(a) => a.apply(query, mb),
            //     GroundSymbolizable::SymbolVar(sv) => sv.apply(query, mb),
            //     GroundSymbolizable::Ground(g) => g.apply(query, mb),
            _ => unimplemented!(),
        };
        Ok(symbol)
    }
}

impl GroundSymbolize for GroundSymbolizable {
    fn symbol(&self) -> Option<Concept> {
        None
    }

    fn symbolize(&self, context: &mut crate::GSContext) -> Result<Concept, crate::MBError> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub struct GroundPair {
    left:     Box<GroundSymbolizable>,
    right:    Box<GroundSymbolizable>,
    position: Position,
}

impl GroundPair {
    pub fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::allege);

        let mut ground_symbol_pair = pair.into_inner().next().unwrap().into_inner();

        // According to the grammar, Allege may only contain symbol_pair
        let left = GroundSymbolizable::parse(ground_symbol_pair.next().unwrap(), position.clone())?;
        let right = GroundSymbolizable::parse(ground_symbol_pair.next().unwrap(), position.clone())?;

        Ok(GroundPair { left: Box::new(left),
                        right: Box::new(right),
                        position })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, nest: bool) -> Result<(), std::io::Error> {
        if nest {
            writer.write(b"(")?;
        }

        self.left.write(writer, false, true)?;
        writer.write(b" : ")?;
        self.right.write(writer, false, true)?;

        if nest {
            writer.write(b")")?;
        }
        Ok(())
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<Concept, MBQLError> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub enum Artifact {
    Agent(Agent),
    Url(Url),
    Text(Text),
    DataNode(DataNode),
    DataRelation(DataRelation),
    ArtifactVar(ArtifactVar),
}

impl Artifact {
    pub fn write<T: std::io::Write>(&self, writer: &mut T, verbose: bool) -> Result<(), std::io::Error> {
        match self {
            Artifact::Agent(agent) => agent.write(writer)?,
            Artifact::Url(url) => url.write(writer, false)?,
            Artifact::Text(text) => text.write(writer, verbose)?,
            Artifact::DataNode(node) => node.write(writer)?,
            Artifact::DataRelation(relation) => relation.write(writer)?,
            Artifact::ArtifactVar(var) => var.write(writer)?,
        }
        Ok(())
    }

    pub fn parse(pair: Pair<parse::Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::artifact);
        let child = pair.into_inner().next().unwrap();

        let a = match child.as_rule() {
            Rule::artifactvar => Artifact::ArtifactVar(ArtifactVar::parse(child, position)?),
            Rule::agent => Artifact::Agent(Agent::parse(child, position)?),
            Rule::datanode => Artifact::DataNode(DataNode::parse(child, position)?),
            Rule::datarelation => Artifact::DataRelation(DataRelation::parse(child, position)?),
            Rule::text => Artifact::Text(Text::parse(child, position)?),
            Rule::url => Artifact::Url(Url::parse(child, position)?),
            _ => unreachable!(),
        };

        Ok(a)
    }

    pub fn apply(&self, query: &Query, mb: &MindBase) -> Result<ArtifactId, MBQLError> {
        let artifact_id = match self {
            Artifact::Agent(agent) => mb.put_artifact(agent.get_agent_id(mb)?)?,
            Artifact::Url(url) => mb.put_artifact(crate::artifact::Url { url: url.url.clone() })?,
            Artifact::Text(text) => mb.put_artifact(crate::artifact::Text::new(&text.text))?,
            Artifact::DataNode(node) => {
                let data_type = node.data_type.apply(query, mb)?;
                mb.put_artifact(crate::artifact::DataNode { data_type,
                                                            data: node.data.clone() })?
            },
            // Artifact::DataRelation(relation) => relation.write(writer)?,
            Artifact::ArtifactVar(var) => query.get_artifact_var(var, mb)?,
            _ => unimplemented!(),
        };

        Ok(artifact_id)
    }
}

// impl Into<crate::artifact::Artifact> for Agent {
//     fn into(self) -> crate::artifact::Artifact {
//         crate::artifact::Artifact::Agent(crate::agent::Agent)
//     }
// }

#[derive(Debug)]
pub struct Agent {
    pub(crate) ident: String,
    position:         Position,
}

impl Agent {
    fn parse(pair: Pair<Rule>, position: Position) -> Result<Self, MBQLError> {
        assert_eq!(pair.as_rule(), Rule::agent);
        Ok(Agent { ident: pair.into_inner().next().unwrap().as_str().to_string(),
                   position })
    }

    pub fn write<T: std::io::Write>(&self, mut writer: T) -> Result<(), std::io::Error> {
        writer.write(format!("Agent({})", self.ident).as_bytes())?;
        Ok(())
    }

    pub fn get_agent_id(&self, mb: &MindBase) -> Result<AgentId, MBError> {
        let agent_id = if self.ident == "default" {
            mb.default_agent()?.id()
        } else {
            AgentId::from_base64(&self.ident)?
        };

        Ok(agent_id)
    }
}

#[derive(Debug)]
pub struct Url {
    pub url:      String,
    pub position: Position,
}

impl Url {
    fn parse(pair: Pair<Rule>, position: Position) -> Result<Self, MBQLError> {
        let pair = pair.into_inner().next().unwrap();
        Ok(Self { url: pair.as_str().replace("\\\"", "\""),
                  position })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, _verbose: bool) -> Result<(), std::io::Error> {
        writer.write(format!("Url(\"{}\")", self.url.replace("\"", "\\\"")).as_bytes())?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct Text {
    text:     String,
    position: Position,
}

impl Text {
    fn parse(pair: Pair<Rule>, position: Position) -> Result<Self, MBQLError> {
        let qs = pair.into_inner().next().unwrap();
        let s = qs.into_inner().next().unwrap();

        Ok(Text { text: s.as_str().to_string().replace("\\\"", "\""),
                  position })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T, verbose: bool) -> Result<(), std::io::Error> {
        if verbose {
            writer.write(format!("Text(\"{}\")", self.text.replace("\"", "\\\"")).as_bytes())?;
        } else {
            writer.write(format!("\"{}\"", self.text.replace("\"", "\\\"")).as_bytes())?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct DataNode {
    pub data_type: Box<Symbolizable>,
    pub data:      Option<Vec<u8>>,
    pub position:  Position,
}

impl DataNode {
    fn parse(pair: Pair<Rule>, position: Position) -> Result<Self, MBQLError> {
        let mut inner = pair.into_inner();
        let data_type = Symbolizable::parse(inner.next().unwrap(), position.clone())?;

        let data = match inner.next() {
            Some(next) => {
                match next.as_rule() {
                    Rule::base64 => Some(base64::decode(next.as_str()).unwrap()),
                    Rule::quoted_string => Some(next.as_str().replace("\\\"", "\"").as_bytes().to_owned()),
                    _ => unreachable!(),
                }
            },
            None => None,
        };

        Ok(DataNode { data_type: Box::new(data_type),
                      data,
                      position })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), std::io::Error> {
        writer.write(b"DataNode(")?;
        self.data_type.write(writer, false, false)?;

        if let Some(data) = &self.data {
            writer.write(b";")?;
            let mut enc = base64::write::EncoderWriter::new(writer, base64::STANDARD);
            use std::io::Write;
            enc.write_all(data)?;
        }
        writer.write(b")")?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct DataRelation {
    pub relation_type: Box<Symbolizable>,
    pub from:          Box<Symbolizable>,
    pub to:            Box<Symbolizable>,
    pub position:      Position,
}

impl DataRelation {
    fn parse(pair: Pair<Rule>, position: Position) -> Result<Self, MBQLError> {
        let mut inner = pair.into_inner();

        let relation_type = Symbolizable::parse(inner.next().unwrap(), position.clone())?;
        let from = Symbolizable::parse(inner.next().unwrap(), position.clone())?;
        let to = Symbolizable::parse(inner.next().unwrap(), position.clone())?;

        Ok(DataRelation { relation_type: Box::new(relation_type),
                          from: Box::new(from),
                          to: Box::new(to),
                          position })
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T) -> Result<(), std::io::Error> {
        writer.write(b"DataRelation(")?;
        self.relation_type.write(writer, false, false)?;
        writer.write(b";")?;

        self.from.write(writer, false, false)?;
        writer.write(b" > ")?;

        self.to.write(writer, false, false)?;
        writer.write(b")")?;

        Ok(())
    }
}
