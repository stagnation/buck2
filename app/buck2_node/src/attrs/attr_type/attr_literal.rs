/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::fmt::Display;

use allocative::Allocative;
use buck2_core::buck_path::path::BuckPathRef;
use buck2_core::package::PackageLabel;
use buck2_util::arc_str::ArcSlice;
use buck2_util::arc_str::ArcStr;
use dupe::Dupe;
use either::Either;
use gazebo::prelude::*;
use static_assertions::assert_eq_size;

use super::attr_config::CoercedAttrExtraTypes;
use super::attr_config::ConfiguredAttrExtraTypes;
use crate::attrs::attr_type::arg::StringWithMacros;
use crate::attrs::attr_type::attr_config::AttrConfig;
use crate::attrs::attr_type::attr_config::AttrConfigExtraTypes;
use crate::attrs::attr_type::configuration_dep::ConfigurationDepAttrType;
use crate::attrs::attr_type::configured_dep::ExplicitConfiguredDepAttrType;
use crate::attrs::attr_type::dep::DepAttrType;
use crate::attrs::attr_type::label::LabelAttrType;
use crate::attrs::attr_type::query::QueryAttr;
use crate::attrs::attr_type::split_transition_dep::SplitTransitionDepAttrType;
use crate::attrs::coerced_attr::CoercedAttr;
use crate::attrs::coerced_path::CoercedPath;
use crate::attrs::configuration_context::AttrConfigurationContext;
use crate::attrs::configured_attr::ConfiguredAttr;
use crate::attrs::configured_traversal::ConfiguredAttrTraversal;
use crate::attrs::display::AttrDisplayWithContext;
use crate::attrs::display::AttrDisplayWithContextExt;
use crate::attrs::fmt_context::AttrFmtContext;
use crate::attrs::traversal::CoercedAttrTraversal;
use crate::visibility::VisibilitySpecification;

#[derive(Debug, Eq, PartialEq, Hash, Clone, Allocative)]
pub enum AttrLiteral<C: AttrConfig> {
    Bool(bool),
    Int(i32),
    // Note we store `String`, not `Arc<str>` here, because we store full attributes
    // in unconfigured target node, but configured target node is basically a pair
    // (reference to unconfigured target node, configuration).
    //
    // Configured attributes are created on demand and destroyed immediately after use.
    //
    // So when working with configured attributes with pay with CPU for string copies,
    // but don't increase total memory usage, because these string copies are short living.
    String(ArcStr),
    // Like String, but drawn from a set of variants, so doesn't support concat
    EnumVariant(ArcStr),
    List(ArcSlice<C>),
    Tuple(ArcSlice<C>),
    Dict(ArcSlice<(C, C)>),
    None,
    Query(Box<QueryAttr<C>>),
    SourceFile(CoercedPath),
    Arg(StringWithMacros<C>),
    // NOTE: unlike deps, labels are not traversed, as they are typically used in lieu of deps in
    // cases that would cause cycles.
    OneOf(
        Box<Self>,
        // Index of matched oneof attr type variant.
        u32,
    ),
    Visibility(VisibilitySpecification),
    Extra(C::ExtraTypes),
}

// Prevent size regression.
assert_eq_size!(AttrLiteral<CoercedAttr>, [usize; 3]);

impl<C: AttrConfig> AttrDisplayWithContext for AttrLiteral<C> {
    fn fmt(&self, ctx: &AttrFmtContext, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttrLiteral::Bool(v) => {
                let s = if *v { "True" } else { "False" };
                write!(f, "{}", s)
            }
            AttrLiteral::Int(v) => {
                write!(f, "{}", v)
            }
            AttrLiteral::String(v) | AttrLiteral::EnumVariant(v) => {
                if f.alternate() {
                    f.write_str(v)
                } else {
                    write!(f, "\"{}\"", v)
                }
            }
            AttrLiteral::List(list) => {
                write!(f, "[")?;
                for (i, v) in list.iter().enumerate() {
                    if i != 0 {
                        write!(f, ",")?;
                    }
                    AttrDisplayWithContext::fmt(v, ctx, f)?;
                }
                write!(f, "]")?;
                Ok(())
            }
            AttrLiteral::Tuple(v) => {
                write!(f, "(")?;
                for (i, v) in v.iter().enumerate() {
                    if i != 0 {
                        write!(f, ",")?;
                    }
                    AttrDisplayWithContext::fmt(v, ctx, f)?;
                }
                write!(f, ")")?;
                Ok(())
            }
            AttrLiteral::Dict(v) => {
                write!(f, "{{")?;
                for (i, (k, v)) in v.iter().enumerate() {
                    if i != 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{}: {}", k.as_display(ctx), v.as_display(ctx))?;
                }
                write!(f, "}}")?;
                Ok(())
            }
            AttrLiteral::None => write!(f, "None"),
            AttrLiteral::Query(v) => write!(f, "\"{}\"", v.query()),
            AttrLiteral::SourceFile(v) => write!(f, "\"{}\"", Self::source_file_display(ctx, v)),
            AttrLiteral::Arg(a) => write!(f, "\"{}\"", a),
            AttrLiteral::OneOf(box l, _) => AttrDisplayWithContext::fmt(l, ctx, f),
            AttrLiteral::Visibility(v) => Display::fmt(v, f),
            AttrLiteral::Extra(u) => Display::fmt(u, f),
        }
    }
}

impl<C: AttrConfig> AttrLiteral<C> {
    fn source_file_display<'a>(
        ctx: &'a AttrFmtContext,
        source_file: &'a CoercedPath,
    ) -> impl Display + 'a {
        match &ctx.package {
            Some(pkg) => Either::Left(BuckPathRef::new(pkg.dupe(), source_file.path())),
            None => {
                // This code is unreachable, but better this than panic.
                Either::Right(format!("<no package>/{}", source_file.path()))
            }
        }
    }

    pub fn to_json(&self, ctx: &AttrFmtContext) -> anyhow::Result<serde_json::Value> {
        use serde_json::to_value;
        match self {
            AttrLiteral::Bool(v) => Ok(to_value(v)?),
            AttrLiteral::Int(v) => Ok(to_value(v)?),
            AttrLiteral::String(v) | AttrLiteral::EnumVariant(v) => Ok(to_value(v)?),
            AttrLiteral::List(list) | AttrLiteral::Tuple(list) => {
                Ok(to_value(list.try_map(|c| c.to_json(ctx))?)?)
            }
            AttrLiteral::Dict(dict) => {
                let mut res: serde_json::Map<String, serde_json::Value> =
                    serde_json::Map::with_capacity(dict.len());
                for (k, v) in &**dict {
                    res.insert(
                        k.to_json(ctx)?.as_str().unwrap().to_owned(),
                        v.to_json(ctx)?,
                    );
                }
                Ok(res.into())
            }
            AttrLiteral::None => Ok(serde_json::Value::Null),
            AttrLiteral::Query(q) => Ok(to_value(q.query())?),
            AttrLiteral::SourceFile(s) => {
                Ok(to_value(Self::source_file_display(ctx, s).to_string())?)
            }
            AttrLiteral::Arg(a) => Ok(to_value(a.to_string())?),
            AttrLiteral::OneOf(box l, _) => l.to_json(ctx),
            AttrLiteral::Visibility(v) => Ok(v.to_json()),
            AttrLiteral::Extra(u) => u.to_json(),
        }
    }

    /// Checks if this attr matches the filter. For container-like things, will return true if any contained item matches the filter.
    pub fn any_matches(
        &self,
        filter: &dyn Fn(&str) -> anyhow::Result<bool>,
    ) -> anyhow::Result<bool> {
        match self {
            AttrLiteral::String(v) | AttrLiteral::EnumVariant(v) => filter(v),
            AttrLiteral::List(vals) | AttrLiteral::Tuple(vals) => {
                for v in vals.iter() {
                    if v.any_matches(filter)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            AttrLiteral::Dict(d) => {
                for (k, v) in &**d {
                    if k.any_matches(filter)? || v.any_matches(filter)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            AttrLiteral::None => Ok(false),
            AttrLiteral::SourceFile(s) => filter(&s.path().to_string()),
            AttrLiteral::Query(q) => filter(q.query()),
            AttrLiteral::Arg(a) => filter(&a.to_string()),
            AttrLiteral::Bool(b) => filter(if *b { "True" } else { "False" }),
            AttrLiteral::Int(i) => filter(&i.to_string()),
            AttrLiteral::OneOf(l, _) => l.any_matches(filter),
            AttrLiteral::Visibility(v) => match v {
                VisibilitySpecification::Public => filter("PUBLIC"),
                VisibilitySpecification::Default => filter(":"),
                VisibilitySpecification::VisibleTo(patterns) => {
                    for p in &***patterns {
                        if filter(&p.to_string())? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
            },
            AttrLiteral::Extra(d) => d.any_matches(filter),
        }
    }
}

impl AttrLiteral<ConfiguredAttr> {
    pub(crate) fn traverse<'a>(
        &'a self,
        pkg: PackageLabel,
        traversal: &mut dyn ConfiguredAttrTraversal<'a>,
    ) -> anyhow::Result<()> {
        match self {
            AttrLiteral::Bool(_) => Ok(()),
            AttrLiteral::Int(_) => Ok(()),
            AttrLiteral::String(_) => Ok(()),
            AttrLiteral::EnumVariant(_) => Ok(()),
            AttrLiteral::List(list) | AttrLiteral::Tuple(list) => {
                for v in list.iter() {
                    v.traverse(pkg.dupe(), traversal)?;
                }
                Ok(())
            }
            AttrLiteral::Dict(dict) => {
                for (k, v) in &**dict {
                    k.traverse(pkg.dupe(), traversal)?;
                    v.traverse(pkg.dupe(), traversal)?;
                }
                Ok(())
            }
            AttrLiteral::None => Ok(()),
            AttrLiteral::Query(query) => query.traverse(traversal),
            AttrLiteral::SourceFile(source) => {
                for x in source.inputs() {
                    traversal.input(BuckPathRef::new(pkg.dupe(), x))?;
                }
                Ok(())
            }
            AttrLiteral::Arg(arg) => arg.traverse(traversal),
            AttrLiteral::OneOf(l, _) => l.traverse(pkg, traversal),
            AttrLiteral::Visibility(..) => Ok(()),
            AttrLiteral::Extra(u) => match u {
                ConfiguredAttrExtraTypes::ExplicitConfiguredDep(dep) => {
                    dep.as_ref().traverse(traversal)
                }
                ConfiguredAttrExtraTypes::SplitTransitionDep(deps) => {
                    for target in deps.deps.values() {
                        traversal.dep(target)?;
                    }
                    Ok(())
                }
                ConfiguredAttrExtraTypes::ConfigurationDep(dep) => traversal.configuration_dep(dep),
                ConfiguredAttrExtraTypes::Dep(dep) => dep.traverse(traversal),
                ConfiguredAttrExtraTypes::SourceLabel(dep) => traversal.dep(dep),
                ConfiguredAttrExtraTypes::Label(label) => traversal.label(label),
            },
        }
    }
}

impl AttrLiteral<CoercedAttr> {
    pub fn configure(&self, ctx: &dyn AttrConfigurationContext) -> anyhow::Result<ConfiguredAttr> {
        Ok(ConfiguredAttr(match self {
            AttrLiteral::Bool(v) => AttrLiteral::Bool(*v),
            AttrLiteral::Int(v) => AttrLiteral::Int(*v),
            AttrLiteral::String(v) => AttrLiteral::String(v.dupe()),
            AttrLiteral::EnumVariant(v) => AttrLiteral::EnumVariant(v.dupe()),
            AttrLiteral::List(list) => {
                AttrLiteral::List(list.try_map(|v| v.configure(ctx))?.into())
            }
            AttrLiteral::Tuple(list) => {
                AttrLiteral::Tuple(list.try_map(|v| v.configure(ctx))?.into())
            }
            AttrLiteral::Dict(dict) => AttrLiteral::Dict(
                dict.try_map(|(k, v)| {
                    let k2 = k.configure(ctx)?;
                    let v2 = v.configure(ctx)?;
                    anyhow::Ok((k2, v2))
                })?
                .into(),
            ),
            AttrLiteral::None => AttrLiteral::None,
            AttrLiteral::Query(query) => AttrLiteral::Query(Box::new(query.configure(ctx)?)),
            AttrLiteral::SourceFile(s) => AttrLiteral::SourceFile(s.clone()),
            AttrLiteral::Arg(arg) => AttrLiteral::Arg(arg.configure(ctx)?),
            AttrLiteral::OneOf(l, i) => {
                let ConfiguredAttr(configured) = l.configure(ctx)?;
                AttrLiteral::OneOf(Box::new(configured), *i)
            }
            AttrLiteral::Visibility(v) => AttrLiteral::Visibility(v.clone()),
            AttrLiteral::Extra(u) => match u {
                CoercedAttrExtraTypes::ExplicitConfiguredDep(dep) => {
                    ExplicitConfiguredDepAttrType::configure(ctx, dep)?
                }
                CoercedAttrExtraTypes::SplitTransitionDep(dep) => {
                    SplitTransitionDepAttrType::configure(ctx, dep)?
                }
                CoercedAttrExtraTypes::ConfiguredDep(dep) => {
                    AttrLiteral::Extra(ConfiguredAttrExtraTypes::Dep(dep.clone()))
                }
                CoercedAttrExtraTypes::ConfigurationDep(dep) => {
                    ConfigurationDepAttrType::configure(ctx, dep)?
                }
                CoercedAttrExtraTypes::Dep(dep) => DepAttrType::configure(ctx, dep)?,
                CoercedAttrExtraTypes::SourceLabel(source) => {
                    AttrLiteral::Extra(ConfiguredAttrExtraTypes::SourceLabel(Box::new(
                        source.configure_pair(ctx.cfg().cfg_pair().dupe()),
                    )))
                }
                CoercedAttrExtraTypes::Label(label) => LabelAttrType::configure(ctx, label)?,
            },
        }))
    }

    pub(crate) fn traverse<'a>(
        &'a self,
        pkg: PackageLabel,
        traversal: &mut dyn CoercedAttrTraversal<'a>,
    ) -> anyhow::Result<()> {
        match self {
            AttrLiteral::Bool(_) => Ok(()),
            AttrLiteral::Int(_) => Ok(()),
            AttrLiteral::String(_) => Ok(()),
            AttrLiteral::EnumVariant(_) => Ok(()),
            AttrLiteral::List(list) | AttrLiteral::Tuple(list) => {
                for v in list.iter() {
                    v.traverse(pkg.dupe(), traversal)?;
                }
                Ok(())
            }
            AttrLiteral::Dict(dict) => {
                for (k, v) in &**dict {
                    k.traverse(pkg.dupe(), traversal)?;
                    v.traverse(pkg.dupe(), traversal)?;
                }
                Ok(())
            }
            AttrLiteral::None => Ok(()),
            AttrLiteral::Query(query) => query.traverse(traversal),
            AttrLiteral::SourceFile(source) => {
                for x in source.inputs() {
                    traversal.input(BuckPathRef::new(pkg.dupe(), x))?;
                }
                Ok(())
            }
            AttrLiteral::Arg(arg) => arg.traverse(traversal),
            AttrLiteral::OneOf(box l, _) => l.traverse(pkg, traversal),
            AttrLiteral::Visibility(..) => Ok(()),
            AttrLiteral::Extra(u) => match u {
                CoercedAttrExtraTypes::ExplicitConfiguredDep(dep) => dep.traverse(traversal),
                CoercedAttrExtraTypes::SplitTransitionDep(dep) => {
                    traversal.split_transition_dep(dep.label.target(), &dep.transition)
                }
                CoercedAttrExtraTypes::ConfiguredDep(dep) => {
                    traversal.dep(dep.label.target().unconfigured())
                }
                CoercedAttrExtraTypes::ConfigurationDep(dep) => traversal.configuration_dep(dep),
                CoercedAttrExtraTypes::Dep(dep) => dep.traverse(traversal),
                CoercedAttrExtraTypes::SourceLabel(s) => traversal.dep(s.target()),
                CoercedAttrExtraTypes::Label(label) => traversal.label(label),
            },
        }
    }
}
