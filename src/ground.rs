// The following MBQL statement
// $emote = Ground(("Smile" : "Mouth") : ("Wink" : "Eye"))
//
// generates this AST Structure:
//                           Ground
//                             |
//                    GSymbolizable::GroundPair
//                                 |
//                GroundPair{ left    right    }
//                            /          \
//       GSymbolizable::GroundPair       GSymbolizable::GroundPair
//                 |                                    |
//       GPair{left right}                    GPair{left right}
//             /        \                         /        \
//    GSym::Artifact  GSym::Artifact   GSym::Artifact  GSym::Artifact
//                |           |             |           |
// Which should   |           |             |           |
// Result in      |           |             |           |
// these symbol   |           |             |           |
// being created: |           |             |           |
//                |           |             |           |
//         # S1[Smile]   S2[Mouth]      S3[Wink]    S4[Eye] <~> [Eye]
//         #  \__________/               \__________/
//         #      [A1]                       [A2]
//         #        \_________________________/
//         #                    [A3] (Emotive Movement)
//
//
// Game plan:
// * Recursively walk the AST
// * left / right Depth first
// * Given a symbol, use that symbol as is for comparison
// * Upon hitting an artifact, Look up all atoms (AllegationID) for the left and right item, filtered by ground agent
// * identify the set of Allegations which intersect(left0, left1) AND intersect(right0, right1) NOTE: how do we ensure that
//   intersect(left0, right1) AND intersect(right0, left1) are also checked?
// * The set of those passing allegations is returned as a Symbol

// Index `atoms_by_artifact_agent` indexes ALL allegations, keyed on ArtifactID + AgentID returned for `referenced_artifacts`
// which is vicarious for analogies (how many levels removed?)
// This might be doing close to what we want already
//
// Index `analogy_rev` contains only analogies (AllegationID) keyed on left-hand symbol atoms (AllegationIDs)
// I'm not certain if this is useful for much

// TODO 2 - rewrite this explanation:
// * recurse to the leaf
//   * get a full scoop  for {left, right}
//   * identify the atoms which alledge BOTH of these
//     * recurse
// * If any branch runs dry, we have to vivify it all the way to the leaf... but we can't use the full scoops we gathered for each
//   leaf
//   * we have to create new symbols which are just for this

use crate::{
    allegation::{
        Allegation,
        Body,
    },
    mbql::{
        ast,
        error::{
            MBQLError,
            MBQLErrorKind,
        },
        query::BindResult,
        Query,
    },
    symbol::Atom,
    AgentId,
    AllegationId,
    Analogy,
    ArtifactId,
    MBError,
    MindBase,
    Symbol,
};
use std::convert::TryInto;

use std::rc::Rc;

pub struct GSContext<'a> {
    scan_min:  [u8; 64],
    scan_max:  [u8; 64],
    gs_agents: Vec<AgentId>,
    mb:        &'a MindBase,
}

impl<'a> GSContext<'a> {
    pub fn new(mb: &'a MindBase) -> Self {
        let gs_agents = mb.ground_symbol_agents.lock().unwrap().clone();

        let mut scan_min: [u8; 64] = [0; 64];
        scan_min[32..64].copy_from_slice(gs_agents.first().unwrap().as_ref());
        let mut scan_max: [u8; 64] = [0; 64];
        scan_max[32..64].copy_from_slice(gs_agents.last().unwrap().as_ref());

        Self { scan_min,
               scan_max,
               gs_agents,
               mb }
    }

    // TODO 3 - come up with a better name for this
    pub fn raw_symbolize<T>(&self, thing: T) -> Result<Symbol, MBError>
        where T: crate::allegation::Alledgable
    {
        self.mb.symbolize(thing)
    }

    /// Call this with the top level GSymbolizable within a ground symbol statement
    /// The goal here is to try to resolve upon the most precise symbolic definition possible, and
    /// arrive at a "ground symbol" which we hope is able to bridge the gap between `External Meaning` and `Internal Meaning`
    ///
    /// The list of ground agents are important, because they represent the starting point of common, culturally originated
    /// defintions in the form of "default" Analogies, which the agent would otherwise have to define for themselves. The
    /// agent in question could theoretically define all of this themselves, but it would be very time consuming, and
    /// crucially, it would impede rather than seed convergence with their neighbors - unless those neighbors first accepted said
    /// agent to be a grounding/neighbor agent. This is of course the goal: that you should ascribe, at least in part, to the set
    /// of definitions which is provided by your neighbor. This is because it reflects ontological alignments which exist in
    /// the real world, at least to some degree.
    ///
    /// This list of artifacts is taken to be a single thread of a taxonomy. Each artifact is initially translated into
    /// the the broadest possible Symbol which is inclusive of _all_ potential interpretations of that artifact.
    /// The initial Symbol of that taxonomy is not able to be narrowed, but the subsequent symbols in the taxonomy are narrowed
    /// to include only those which are alledged to be in the category of the parent by one of the grounding/neighbor agents.
    ///
    /// This in theory should allow us to resolve upon a single symbol which is believed to be meaningful to that agent based on
    /// the artifacts they posess. This is our interface between the physical world, and the perpetually-convergent ontological
    /// continuum we hope to create with mindbase.
    pub fn symbolize(&mut self, symbolizable: &Rc<ast::GSymbolizable>, vivify: bool, query: &Query) -> Result<Symbol, MBQLError> {
        // As a temporary measure, we are doing a fairly inefficient process of building a Symbol for each symbolizable artifact
        // with all possible symbolic atoms and THEN narrowing that.
        //
        // Later, we should be able to improve this with strategic indexing such that the narrowing step is less burdensome (or
        // even unnecessary) and that roundtripping to the data storage layer is reduced

        // TODO - create a shared context which can be used for a rolling index intersection process
        // TODO - change this to not return a symbol, but rather to mutate the context
        let node = self.symbolize_recurse(symbolizable, vivify, query)?;

        // TODO - convert the rolling index intersection into a symbol and Return.

        Ok(node.take_symbol())
    }

    fn symbolize_recurse(&mut self, gsym: &Rc<ast::GSymbolizable>, vivify: bool, query: &Query) -> Result<GSNode, MBQLError> {
        let symbol = match &**gsym {
            ast::GSymbolizable::Artifact(a) => GSNode::artifact(self, vivify, query, a)?,
            ast::GSymbolizable::GroundPair(a) => GSNode::pair(self, vivify, query, a)?,
            ast::GSymbolizable::SymbolVar(sv) => GSNode::symbolvar(self, vivify, query, sv)?,

            ast::GSymbolizable::Ground(_) => {
                // Shouldn't be able to call this directly with a Ground statement
                unreachable!()
            },
        };

        Ok(symbol)
    }

    fn unrefined_symbol_for_artifact(&mut self, search_artifact_id: &ArtifactId) -> Result<Option<Symbol>, MBError> {
        self.scan_min[0..32].copy_from_slice(search_artifact_id.as_ref());
        self.scan_max[0..32].copy_from_slice(search_artifact_id.as_ref());

        let iter = self.mb.atoms_by_artifact_agent.range(&self.scan_min[..]..=&self.scan_max[..]);

        use inverted_index_util::entity_list::insert_entity_mut;

        use typenum::consts::U16;
        let mut unified: Vec<u8> = Vec::new();
        for item in iter {
            let (key, atom_list) = item?;
            // atom_list is a Vec[u8] containing a sorted sequence of 16 bit atom ids

            // TODO - differentiate (keys or list items) based on the type and vicariousness of artifact -> atom
            // Is this a direct symbolization of that artifact? or an Analogy?
            // At present, we are only indexing direct symbolizations, so we can cheat and skip this

            let item_agent_id = &key[32..64];
            // Remember we're searching for a range of agent ids. Have to confirm it's in the list
            if let Err(_) = self.gs_agents.binary_search_by(|a| a.as_ref()[..].cmp(item_agent_id)) {
                // No, it's not present in the list. Punt
                continue;
            }

            if unified.len() == 0 {
                unified.extend(&atom_list[..])
            } else {
                for chunk in atom_list.chunks(16) {
                    insert_entity_mut::<U16>(&mut unified, chunk)
                }
            }
        }

        let atoms: Vec<Atom> = unified.chunks_exact(16)
                                      .map(|c| Atom::up(AllegationId::from_bytes(c.try_into().unwrap())))
                                      .collect();
        Ok(Symbol::new_option(atoms))
    }

    // It's not really just one analogy that we're searching for, but a collection of N analogies which match left and right
    fn find_matching_analogy_symbol(&self, left: &GSNode, right: &GSNode, query: &Query) -> Result<Option<Symbol>, MBError> {
        // Brute force for now. This whole routine is insanely inefficient
        // TODO 2 - update this to be a sweet indexed query!

        // Three buckets
        // 1. Matching Analogies
        // 2. Left matches
        // 3. Right matches

        // The stupid way that's slightly less horrible than what we're doing now:
        // * Pre-Index Analogies by all AllegationIds directly referenced (Inverted index of AllegationID -> Vec<AnalogyId>)
        // * Perform a lexicographic merge of left and right Symbols into compare_list
        // * iterate over compare_list and query once per each atom (ugh) *

        // The Reeally stupid way:
        // * lexmerge left and right inputs
        // * create three empty output lists (target, left, right)
        // * iterate over all Analogies
        //   * lexmerge analogy left+right
        //   * intersect the two lists
        //   * output must have at least two entries
        //   * intersect lexmerged analogy again with left (intersection)

        // 1 1
        // 1 2
        // 1 9
        // 2 3
        // 2 4

        // [(1,2), (2,3), (3,3)]

        let left = left.symbol();
        let right = right.symbol();

        let comp_merged: Vec<SidedMergeItem<Atom>> = SidedMerge::new(left.atoms.iter(), right.atoms.iter()).map(|a| a.to_owned())
                                                                                                           .collect();

        let output_left: Vec<Atom> = Vec::new();
        let output_right: Vec<Atom> = Vec::new();
        let output_analogy: Vec<Atom> = Vec::new();

        // TODO 2 - This is crazy inefficient
        let iter = query.mb.allegation_iter().filter_map(|allegation| {
                                                 match allegation {
                                                     Ok((_,
                                                         Allegation { body: Body::Analogy(analogy),
                                                                      agent_id,
                                                                      .. }))
                                                         if self.gs_agents.contains(&agent_id) =>
                                                     {
                                                         Some(Ok(analogy))
                                                     },
                                                     Ok(_) => None,
                                                     Err(e) => Some(Err(e)),
                                                 }
                                             });

        for analogy in iter {
            let analogy = analogy?;

            // TODO 1 - consider storing analogies this way instead of Symbol, Symbol.
            // EG: vec![ Left(Atom), Right(Atom), Right(Atom) ]
            // This *Might* also help with Catagorical analogies:
            // or vec![Categorical(Atom), Categorical(Atom)] meaning that both atoms are in the same category - ((should mixing be
            // allowed??))
            let analogy_merged = SidedMerge::new(analogy.left.atoms.iter(), analogy.right.atoms.iter());

            let si = SortedIntersect::new(analogy_merged, comp_merged.iter());

            // This is one Analogy we're dealing with here

            for item in si {
                // Left off here. This is rough, but it's close in theory
                // if I get a hit on left/left, what sort of filtering do I have to do for correctness?

                // can any left/left hit I get go straight into the left-handed bucket?
                // can any right/right hit I get got straight into the right-handed bucket?
                // what do I do with left/right and right/left hits?

                match (item.left.side, item.right.side) {
                    // left side of the IntersectItem is itself a SidedMerge item
                    // This works IF we're not iterating over the mirrored copies, I think
                    (Left, Left) => ll_hit = true,
                    (Right, Right) => rr_hit = true,
                    (Left, Right) => lr_hit = true,
                    (Right, Left) => rl_hit = true,
                }
            }

            if (ll_hit && rr_hit) || (lr_hit && rl_hit) {
                output_analogy.push(analogy.id)
            }

            // I need:
            // left1 ⋂ left2 && right1 ⋂ right2
            // OR
            // left1 ⋂ right2 && right1 ⋂ left2

            // for analogy_item in analogy_merged {

            // match ((_,_)) {
            //     (ItemSide::Left,ItemSide::Left) => {
            //         //
            //     },
            //     ItemSide::Right => {
            //         //
            //     },
            // }
            // }
            // if intersect_symbols(&left, &analogy.left) && intersect_symbols(&right, &analogy.right) {
            //     atoms.push(Atom::up(allegation_id))
            // } else if intersect_symbols(&left, &analogy.right) && intersect_symbols(&right, &analogy.left) {
            //     atoms.push(Atom::down(allegation_id))
            // }
        }

        // Hah - this is where we need to call query.store_symbol_for_var
        // Because the symbol is getting narrowed, not novel'ed

        // Create a Symbol which contains the composite symbol atoms of all Analogies made by ground symbol agents
        return Ok(Symbol::new_option(output_analogy));
    }
}

use std::{
    cmp::Ordering,
    iter::Peekable,
};

struct SidedMerge<L, R>
    where L: Iterator<Item = R::Item>,
          R: Iterator
{
    left:  Peekable<L>,
    right: Peekable<R>,
}

impl<L, R> SidedMerge<L, R>
    where L: Iterator<Item = R::Item>,
          R: Iterator
{
    // TODO 2 - Consider creating a marker trait for attestation that the iterator is pre-sorted? (Ascending)

    fn new(left: L, right: R) -> Self {
        SidedMerge { left:  left.peekable(),
                     right: right.peekable(), }
    }
}

pub struct SidedMergeItem<T> {
    pub item: T,
    side:     ItemSide,
}
enum ItemSide {
    Left,
    Right,
}

impl<T: Clone> SidedMergeItem<&T> {
    pub fn to_owned(self) -> SidedMergeItem<T> {
        SidedMergeItem { item: self.item.clone(),
                         side: self.side, }
    }
}

impl<L, R> Iterator for SidedMerge<L, R>
    where L: Iterator<Item = R::Item>,
          R: Iterator,
          L::Item: Ord
{
    type Item = SidedMergeItem<L::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        let which = match (self.left.peek(), self.right.peek()) {
            (Some(l), Some(r)) => Some(l.cmp(r)),
            (Some(_), None) => Some(Ordering::Less),
            (None, Some(_)) => Some(Ordering::Greater),
            (None, None) => None,
        };

        match which {
            Some(Ordering::Less) => {
                Some(SidedMergeItem { item: self.left.next().unwrap(),
                                      side: ItemSide::Left, })
            },
            Some(Ordering::Equal) => {
                Some(SidedMergeItem { item: self.left.next().unwrap(),
                                      side: ItemSide::Left, })
            },
            Some(Ordering::Greater) => {
                Some(SidedMergeItem { item: self.right.next().unwrap(),
                                      side: ItemSide::Right, })
            },
            None => None,
        }
    }
}

struct SortedIntersect<L, R>
    where L: Iterator<Item = R::Item>,
          R: Iterator
{
    left:  Peekable<L>,
    right: Peekable<R>,
}

impl<L, R> SortedIntersect<L, R>
    where L: Iterator<Item = R::Item>,
          R: Iterator
{
    // TODO 2 - Consider creating a marker trait for attestation that the iterator is pre-sorted (Ascending)
    fn new(left: L, right: R) -> Self {
        SortedIntersect { left:  left.peekable(),
                          right: right.peekable(), }
    }
}

impl<L, R> Iterator for SortedIntersect<L, R>
    where L: Iterator<Item = R::Item>,
          R: Iterator,
          L::Item: Ord
{
    type Item = L::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let mut left = match self.left.next() {
            None => return None,
            Some(i) => i,
        };

        let mut right = match self.right.next() {
            None => return None,
            Some(i) => i,
        };

        use std::cmp::Ordering::*;
        loop {
            match left.cmp(&right) {
                Less => {
                    left = match self.left.next() {
                        Some(x) => x,
                        None => return None,
                    };
                },
                Greater => {
                    right = match self.right.next() {
                        Some(x) => x,
                        None => return None,
                    };
                },
                Equal => return Some(left),
            }
        }
    }
}

fn analogy_compare(analogy: &Analogy, left: &Symbol, right: &Symbol, atoms: &mut Vec<Atom>) {
    // use crate::symbol::SpinCompare;

    // let l_iter = left.atoms.iter();
    // let r_iter = right.atoms.iter();
    // let al_iter = analogy.left.atoms.iter();
    // let ar_iter = analogy.right.atoms.iter();

    // // compare l/l + r/r
    // // compare l/r + r/l

    // loop {
    //     // magic lexicographic sort. poof.
    //     let l = l_iter.next();
    //     let r = r_iter.next();
    //     let al = l_iter.next();
    //     let ar = r_iter.next();

    //     if l = al && r = ar {

    //     }else{

    //     }
    // }

    // for member in left.atoms.iter() {}

    // let a =

    //     if intersect_symbols(left, &analogy.left) && intersect_symbols(right, &analogy.right) {
    //         Some(Spin::Up)
    //     } else if intersect_symbols(left, &analogy.right) && intersect_symbols(right, &analogy.left) {
    //         Some(Spin::Down)
    //     }

    //     None
    unimplemented!()
}

fn intersect_symbols(symbol_a: &Symbol, symbol_b: &Symbol) -> bool {
    let mut a_iter = symbol_a.atoms.iter();
    let mut b_iter = symbol_b.atoms.iter();

    let mut a = match a_iter.next() {
        Some(v) => v,
        None => {
            return false;
        },
    };

    let mut b = match b_iter.next() {
        Some(v) => v,
        None => {
            return false;
        },
    };

    use std::cmp::Ordering::*;
    loop {
        match a.cmp(b) {
            Less => {
                a = match a_iter.next() {
                    Some(x) => x,
                    None => return false,
                };
            },
            Greater => {
                b = match b_iter.next() {
                    Some(x) => x,
                    None => return false,
                };
            },
            Equal => return true,
        }
    }
}

enum GSNode {
    // May need to go back and re-symbolize these
    Artifact {
        artifact_id: ArtifactId,
        symbol:      Symbol,
    },
    Pair {
        symbol: Symbol,
        left:   Box<GSNode>,
        right:  Box<GSNode>,
    },

    Bound {
        gsnode: Box<GSNode>,
        sv:     Rc<ast::SymbolVar>,
    },

    // Someone gave us this symbol, and said "use it", so there's nothing to be done
    Given(Symbol),

    // These are done, and thus don't need to contain any child GSNodes
    Created(Symbol),
}

/// Because we are asserting ground symbols, we don't know if it's necessary to create a new symbol until we identify a
/// preexisting symbol at ALL levels of the tree. If we come up dry, we need to go all the way back to the leaves and create new
/// symbols along the way
impl GSNode {
    pub fn artifact(ctx: &mut GSContext, vivify: bool, query: &Query, artifact: &ast::Artifact) -> Result<Self, MBQLError> {
        let artifact_id = artifact.apply(query)?;

        let node = match ctx.unrefined_symbol_for_artifact(&artifact_id)? {
            Some(symbol) => GSNode::Artifact { artifact_id, symbol },
            None => {
                if vivify {
                    let symbol = ctx.raw_symbolize(artifact_id)?;
                    GSNode::Created(symbol)
                } else {
                    return Err(MBQLError { position: artifact.position().clone(),
                                           kind:     MBQLErrorKind::GSymNotFound, });
                }
            },
        };

        Ok(node)
    }

    pub fn symbolvar(ctx: &mut GSContext, vivify: bool, query: &Query, sv: &Rc<ast::SymbolVar>) -> Result<Self, MBQLError> {
        match query.bind_symbolvar(&sv.var) {
            Err(e) => {
                return Err(MBQLError { position: sv.position().clone(),
                                       kind:     MBQLErrorKind::SymbolVarNotFound { var: sv.var.to_string() }, });
            },
            Ok(BindResult::Bound(gsymz)) => {
                let gsnode = ctx.symbolize_recurse(&gsymz, vivify, query)?;

                println!("Storing symbol for Bound {}", gsnode.symbol());
                // Store the initial symbol we found
                query.store_symbol_for_var(&sv, gsnode.symbol().clone())?;

                Ok(GSNode::Bound { gsnode: Box::new(gsnode),
                                   sv:     sv.clone(), })
            },
            Ok(BindResult::Symbol(symbol)) => Ok(GSNode::Given(symbol)),
        }
    }

    pub fn pair(ctx: &mut GSContext, vivify: bool, query: &Query, gpair: &ast::GPair) -> Result<Self, MBQLError> {
        // Symbol grounding is the crux of the biscuit
        // We don't want to create new symbols if we can possibly help it
        // We want to try reeally hard to find existing symbols
        // And only create a new one if we positively must

        // Depth first recursion to find possible leaf symbols
        let left = ctx.symbolize_recurse(&gpair.left, vivify, query)?;
        let right = ctx.symbolize_recurse(&gpair.right, vivify, query)?;

        // find symbols (Analogies) which refer to BOTH of the above

        // I'm searching for Analogies which match both the left and the right
        // AND I'm also searching for that set of left/right atoms which match said analogies, which I need to call
        // store_symbol_for_var on if they're GSNode::Bound
        let opt_symbol = ctx.find_matching_analogy_symbol(&left, &right, query)?;

        if let Some(symbol) = opt_symbol {
            println!("FOUND MATCH {}", symbol);
            return Ok(GSNode::Pair { symbol,
                                     left: Box::new(left),
                                     right: Box::new(right) });
        }

        if vivify {
            // Didn't find any such analogies, so none of the symbols we found were satisfactory.
            // Lets create a tree of novel symbols which we will now declare as having the ground meaning intended
            let symbol =
                ctx.raw_symbolize(Analogy::declarative(left.novel_symbol(ctx, query)?, right.novel_symbol(ctx, query)?))?;

            // Doesn't matter how we got here
            println!("VIVIFY {}", symbol);

            Ok(GSNode::Created(symbol))
        } else {
            Err(MBQLError { position: gpair.position().clone(),
                            kind:     MBQLErrorKind::GSymNotFound, })
        }
    }

    pub fn symbol(&self) -> &Symbol {
        match self {
            GSNode::Artifact { symbol, .. } => symbol,
            GSNode::Pair { symbol, .. } => symbol,
            GSNode::Given(symbol) => symbol,
            GSNode::Created(symbol) => symbol,
            GSNode::Bound { gsnode, .. } => gsnode.symbol(),
        }
    }

    pub fn take_symbol(self) -> Symbol {
        match self {
            GSNode::Artifact { symbol, .. } => symbol,
            GSNode::Pair { symbol, .. } => symbol,
            GSNode::Given(symbol) => symbol,
            GSNode::Created(symbol) => symbol,
            GSNode::Bound { gsnode, .. } => gsnode.take_symbol(),
        }
    }

    pub fn novel_symbol(self, ctx: &GSContext, query: &Query) -> Result<Symbol, MBError> {
        let symbol = match self {
            GSNode::Artifact { artifact_id, .. } => ctx.raw_symbolize(artifact_id)?,
            GSNode::Bound { gsnode, sv } => {
                println!("BOUND novel symbol");

                let symbol = gsnode.novel_symbol(ctx, query)?;

                // Looks like we had to make a new symbol. Overwrite the one we stored during the first phase of the ground symbol
                // search process
                query.store_symbol_for_var(&sv, symbol.clone())?;
                println!("BOUND novel symbol 2");

                symbol
            },
            GSNode::Pair { left, right, .. } => {
                ctx.raw_symbolize(Analogy::declarative(left.novel_symbol(ctx, query)?, right.novel_symbol(ctx, query)?))?
            },
            GSNode::Created(s) => s,
            GSNode::Given(symbol) => symbol,
        };

        Ok(symbol)
    }
}

#[cfg(test)]
mod test {
    use crate::{
        mbql::{
            error::{
                MBQLError,
                MBQLErrorKind,
            },
            Query,
        },
        MindBase,
    };
    use std::io::Cursor;

    #[test]
    fn ground1() -> Result<(), std::io::Error> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdirpath = tmpdir.path();
        let mb = MindBase::open(&tmpdirpath).unwrap();

        let query = mb.query_str(r#"Ground!(("Smile" : "Mouth") : ("Wink" : "Eye"))"#)?;
        match query.apply() {
            Err(MBQLError { kind: MBQLErrorKind::GSymNotFound,
                            .. }) => {
                // This should fail, because we're disallowing vivification
            },
            r @ _ => panic!("Ground symbol vivification is disallowed {:?}", r),
        }

        let query = mb.query_str(
                                 r#"
        $foo = Allege(("Smile" : "Mouth") : ("Wink":"Eye"))
        $bar = Ground!(("Smile" : "Mouth") : ("Wink" : "Eye"))
        Diag($foo, $bar)
        "#,
        )?;

        // This time it should work, because we are alleging it above in what happens to be exactly the right way to be matched by
        // the ground symbol search
        query.apply()?;

        let bogus = query.get_symbol_for_var("bogus")?;
        assert_eq!(bogus, None);

        let foo = query.get_symbol_for_var("foo")?.expect("foo");
        let bar = query.get_symbol_for_var("bar")?.expect("bar");

        assert_eq!(foo, bar);
        assert!(foo.intersects(&bar));

        Ok(())
    }

    #[test]
    fn ground2() -> Result<(), std::io::Error> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdirpath = tmpdir.path();
        let mb = MindBase::open(&tmpdirpath).unwrap();

        let mbql = Cursor::new(r#"$gs = Ground(("Smile" : "Mouth") : ("Wink" : "Eye"))"#);

        let query = Query::new(&mb, mbql)?;
        query.apply()?;

        let _gs = query.get_symbol_for_var("gs")?.expect("gs");

        Ok(())
    }

    #[test]
    fn ground3() -> Result<(), std::io::Error> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdirpath = tmpdir.path();
        let mb = MindBase::open(&tmpdirpath).unwrap();

        let mbql = Cursor::new(
                               r#"
            $foo = Ground(("Smile" : "Mouth") : ("Wink" : "Eye"))
            $bar = Ground(("Smile" : "Mouth") : ("Wink" : "Eye"))
            Diag($foo, $bar)
        "#,
        );

        let query = Query::new(&mb, mbql)?;
        query.apply()?;

        let foo = query.get_symbol_for_var("foo")?.expect("foo");
        let bar = query.get_symbol_for_var("bar")?.expect("bar");

        assert_eq!(foo, bar);
        assert!(foo.intersects(&bar));

        Ok(())
    }

    #[test]
    fn ground4() -> Result<(), std::io::Error> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdirpath = tmpdir.path();
        let mb = MindBase::open(&tmpdirpath).unwrap();

        // $a = Allege("Raggedy Ann": "Ragdoll")
        // $b = Allege("Raggedy Andy": "Ragdoll")

        // WIP: How to validate that we're properly re-symbolizing?
        let query = mb.query_str(
                                 r#"
            $a = Allege("Ragdoll" : "Leopard")
            $b = Allege("Shepherd" : "Wolf")
            $c = Allege($a : $b)
            $x = Ground(("Ragdoll" : "Leopard") : ("Shepherd" : "Wolf"))
            Diag($a, $b, $x)
        "#,
        )?;

        query.apply()?;

        let a = query.get_symbol_for_var("a")?.expect("a");
        let b = query.get_symbol_for_var("b")?.expect("b");
        let x = query.get_symbol_for_var("x")?.expect("x");

        let lr = x.left_right(&mb)?.expect("left/right referents");

        assert_eq!(a, lr.0);
        assert_eq!(b, lr.1);

        // let stdout = std::io::stdout();
        // let handle = stdout.lock();
        // crate::xport::dump_json(&mb, handle).unwrap();

        // let foo = query.get_symbol_var("foo")?.expect("foo");
        // let bar = query.get_symbol_var("bar")?.expect("bar");

        // assert_eq!(foo, bar);
        // assert!(foo.intersects(&bar));

        Ok(())
    }
}
