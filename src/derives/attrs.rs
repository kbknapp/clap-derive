// Copyright 2018 Guillaume Pinot (@TeXitoi) <texitoi@texitoi.eu>,
// Kevin Knapp (@kbknapp) <kbknapp@gmail.com>, and
// Andrew Hobden (@hoverbear) <andrew@hoverbear.org>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// This work was derived from Structopt (https://github.com/TeXitoi/structopt)
// commit#ea76fa1b1b273e65e3b0b1046643715b49bec51f which is licensed under the
// MIT/Apache 2.0 license.

use heck::{CamelCase, KebabCase, MixedCase, ShoutySnakeCase, SnakeCase};
use proc_macro2;
use std::{env, mem};
use syn;

use derives;

/// Default casing style for generated arguments.
pub const DEFAULT_CASING: CasingStyle = CasingStyle::Kebab;

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Kind {
    Arg(Ty),
    Subcommand(Ty),
    FlattenStruct,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Ty {
    Bool,
    Vec,
    Option,
    OptionOption,
    OptionVec,
    Other,
}

#[derive(Debug)]
pub struct Attrs {
    name: String,
    cased_name: String,
    casing: CasingStyle,
    methods: Vec<Method>,
    parser: (Parser, proc_macro2::TokenStream),
    has_custom_parser: bool,
    kind: Kind,
}

#[derive(Debug)]
struct Method {
    name: String,
    args: proc_macro2::TokenStream,
}

#[derive(Debug, PartialEq)]
pub enum Parser {
    FromStr,
    TryFromStr,
    FromOsStr,
    TryFromOsStr,
    FromOccurrences,
}

/// Defines the casing for the attributes long representation.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CasingStyle {
    /// Indicate word boundaries with uppercase letter, excluding the first word.
    Camel,
    /// Keep all letters lowercase and indicate word boundaries with hyphens.
    Kebab,
    /// Indicate word boundaries with uppercase letter, including the first word.
    Pascal,
    /// Keep all letters uppercase and indicate word boundaries with underscores.
    ScreamingSnake,
    /// Keep all letters lowercase and indicate word boundaries with underscores.
    Snake,
    /// Use the original attribute name defined in the code.
    Verbatim,
}

/// Output for the gen_xxx() methods were we need more than a simple stream of tokens.
///
/// The output of a generation method is not only the stream of new tokens but also the attribute
/// information of the current element. These attribute information may contain valuable information
/// for any kind of child arguments.
pub struct GenOutput {
    pub tokens: proc_macro2::TokenStream,
    pub attrs: Attrs,
}

impl ::std::str::FromStr for Parser {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "from_str" => Ok(Parser::FromStr),
            "try_from_str" => Ok(Parser::TryFromStr),
            "from_os_str" => Ok(Parser::FromOsStr),
            "try_from_os_str" => Ok(Parser::TryFromOsStr),
            "from_occurrences" => Ok(Parser::FromOccurrences),
            _ => Err(format!("unsupported parser {}", s)),
        }
    }
}

impl CasingStyle {
    fn translate(&self, input: &str) -> String {
        match *self {
            CasingStyle::Pascal => input.to_camel_case(),
            CasingStyle::Kebab => input.to_kebab_case(),
            CasingStyle::Camel => input.to_mixed_case(),
            CasingStyle::ScreamingSnake => input.to_shouty_snake_case(),
            CasingStyle::Snake => input.to_snake_case(),
            CasingStyle::Verbatim => String::from(input),
        }
    }
}

impl ::std::str::FromStr for CasingStyle {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let name = name.to_camel_case().to_lowercase();

        let case = match name.as_ref() {
            "camel" | "camelcase" => CasingStyle::Camel,
            "kebab" | "kebabcase" => CasingStyle::Kebab,
            "pascal" | "pascalcase" => CasingStyle::Pascal,
            "screamingsnake" | "screamingsnakecase" => CasingStyle::ScreamingSnake,
            "snake" | "snakecase" => CasingStyle::Snake,
            "verbatim" | "verbatimcase" => CasingStyle::Verbatim,
            _ => return Err(format!("unsupported casing: {}", name)),
        };

        Ok(case)
    }
}

impl Attrs {
    fn new(name: String, casing: CasingStyle) -> Self {
        let cased_name = casing.translate(&name);

        Self {
            name,
            cased_name,
            casing,
            methods: vec![],
            parser: (Parser::TryFromStr, quote!(::std::str::FromStr::from_str)),
            has_custom_parser: false,
            kind: Kind::Arg(Ty::Other),
        }
    }
    fn push_str_method(&mut self, name: &str, arg: &str) {
        match (name, arg) {
            ("about", "") | ("version", "") | ("author", "") => {
                let methods = mem::replace(&mut self.methods, vec![]);
                self.methods = methods.into_iter().filter(|m| m.name != name).collect();
            }
            ("name", new_name) => {
                self.name = new_name.into();
                self.cased_name = self.casing.translate(new_name);
            }
            (name, arg) => self.methods.push(Method {
                name: name.to_string(),
                args: quote!(#arg),
            }),
        }
    }

    fn push_attrs(&mut self, attrs: &[syn::Attribute]) {
        use derives::parse::ClapAttr::*;

        for attr in derives::parse::parse_clap_attributes(attrs) {
            match attr {
                Short => {
                    let cased_name = &self.cased_name.clone();
                    self.push_str_method("short", cased_name);
                }

                Long => {
                    let cased_name = &self.cased_name.clone();
                    self.push_str_method("long", cased_name);
                }

                Subcommand => {
                    self.set_kind(Kind::Subcommand(Ty::Other));
                }

                Flatten => {
                    self.set_kind(Kind::FlattenStruct);
                }

                NameLitStr(name, lit) => {
                    self.push_str_method(&name.to_string(), &lit.value());
                }

                NameExpr(name, expr) => self.methods.push(Method {
                    name: name.to_string(),
                    args: quote!(#expr),
                }),

                MethodCall(name, args) => self.methods.push(Method {
                    name: name.to_string(),
                    args: quote!(#args),
                }),

                RenameAll(casing_lit) => {
                    let casing: CasingStyle = {
                        ::std::str::FromStr::from_str(&casing_lit.value())
                            .unwrap_or_else(|error| panic!("{}", error))
                    };

                    self.casing = casing;
                    self.cased_name = self.casing.translate(&self.name);
                }

                Parse(spec) => {
                    self.has_custom_parser = true;
                    self.parser = match spec.parse_func {
                        None => {
                            use self::Parser::*;
                            let parser = spec.kind.to_string().parse().unwrap();
                            let function = match parser {
                                FromStr | FromOsStr => quote!(::std::convert::From::from),
                                TryFromStr => quote!(::std::str::FromStr::from_str),
                                TryFromOsStr => panic!(
                                    "cannot omit parser function name with `try_from_os_str`"
                                ),
                                FromOccurrences => quote!({ |v| v as _ }),
                            };
                            (parser, function)
                        }

                        Some(func) => {
                            let parser = spec.kind.to_string().parse().unwrap();
                            match func {
                                syn::Expr::Path(_) => (parser, quote!(#func)),
                                _ => panic!("`parse` argument must be a function path"),
                            }
                        }
                    }
                }
            }
        }
    }

    fn push_doc_comment(&mut self, attrs: &[syn::Attribute], name: &str) {
        let doc_comments = attrs
            .iter()
            .filter_map(|attr| {
                let path = &attr.path;
                match quote!(#path).to_string() == "doc" {
                    true => attr.interpret_meta(),
                    false => None,
                }
            })
            .filter_map(|attr| {
                use syn::Lit::*;
                use syn::Meta::*;
                if let NameValue(syn::MetaNameValue {
                    ident, lit: Str(s), ..
                }) = attr
                {
                    if ident != "doc" {
                        return None;
                    }
                    let value = s.value();
                    let text = value
                        .trim_start_matches("//!")
                        .trim_start_matches("///")
                        .trim_start_matches("/*!")
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .trim();
                    if text.is_empty() {
                        Some("\n\n".to_string())
                    } else {
                        Some(text.to_string())
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if doc_comments.is_empty() {
            return;
        }
        let merged_lines = doc_comments
            .join(" ")
            .split('\n')
            .map(str::trim)
            .map(str::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        let expected_doc_comment_split = if let Some(content) = doc_comments.get(1) {
            (doc_comments.len() > 2) && (content == &"\n\n")
        } else {
            false
        };

        if expected_doc_comment_split {
            let long_name = String::from("long_") + name;

            self.methods.push(Method {
                name: long_name,
                args: quote!(#merged_lines),
            });

            // Remove trailing whitespace and period from short help, as rustdoc
            // best practice is to use complete sentences, but command-line help
            // typically omits the trailing period.
            let short_arg = doc_comments
                .first()
                .map(String::as_ref)
                .map(str::trim)
                .map_or("", |s| s.trim_end_matches('.'));

            self.methods.push(Method {
                name: name.to_string(),
                args: quote!(#short_arg),
            });
        } else {
            self.methods.push(Method {
                name: name.to_string(),
                args: quote!(#merged_lines),
            });
        }
    }
    pub fn from_struct(attrs: &[syn::Attribute], name: String, argument_casing: CasingStyle) -> Self {
        let mut res = Self::new(name, argument_casing);
        let attrs_with_env = [
            ("version", "CARGO_PKG_VERSION"),
            ("author", "CARGO_PKG_AUTHORS"),
        ];
        attrs_with_env
            .iter()
            .filter_map(|&(m, v)| env::var(v).ok().and_then(|arg| Some((m, arg))))
            .filter(|&(_, ref arg)| !arg.is_empty())
            .for_each(|(name, arg)| {
                let new_arg = if name == "author" {
                    arg.replace(":", ", ")
                } else {
                    arg
                };
                res.push_str_method(name, &new_arg);
            });
        res.push_doc_comment(attrs, "about");
        res.push_attrs(attrs);
        if res.has_custom_parser {
            panic!("parse attribute is only allowed on fields");
        }
        match res.kind {
            Kind::Subcommand(_) => panic!("subcommand is only allowed on fields"),
            Kind::FlattenStruct => panic!("flatten is only allowed on fields"),
            Kind::Arg(_) => res,
        }
    }
    fn ty_from_field(ty: &syn::Type) -> Ty {
        if let syn::Type::Path(syn::TypePath {
            path: syn::Path { ref segments, .. },
            ..
        }) = *ty
        {
            match segments.iter().last().unwrap().ident.to_string().as_str() {
                "bool" => Ty::Bool,
                "Option" => match derives::sub_type(ty).map(Attrs::ty_from_field) {
                    Some(Ty::Option) => Ty::OptionOption,
                    Some(Ty::Vec) => Ty::OptionVec,
                    _ => Ty::Option,
                },
                "Vec" => Ty::Vec,
                _ => Ty::Other,
            }
        } else {
            Ty::Other
        }
    }
    pub fn from_field(field: &syn::Field, struct_casing: CasingStyle) -> Self {
        let name = field.ident.as_ref().unwrap().to_string();
        let mut res = Self::new(name, struct_casing);
        res.push_doc_comment(&field.attrs, "help");
        res.push_attrs(&field.attrs);

        match res.kind {
            Kind::FlattenStruct => {
                if res.has_custom_parser {
                    panic!("parse attribute is not allowed for flattened entry");
                }
                if !res.methods.is_empty() {
                    panic!("methods and doc comments are not allowed for flattened entry");
                }
            }
            Kind::Subcommand(_) => {
                if res.has_custom_parser {
                    panic!("parse attribute is not allowed for subcommand");
                }
                if !res.methods.iter().all(|m| m.name == "help") {
                    panic!("methods in attributes are not allowed for subcommand");
                }

                let ty = Self::ty_from_field(&field.ty);
                match ty {
                    Ty::OptionOption => {
                        panic!("Option<Option<T>> type is not allowed for subcommand");
                    }
                    Ty::OptionVec => {
                        panic!("Option<Vec<T>> type is not allowed for subcommand");
                    }
                    _ => (),
                }

                res.kind = Kind::Subcommand(ty);
            }
            Kind::Arg(_) => {
                let mut ty = Self::ty_from_field(&field.ty);
                if res.has_custom_parser {
                    match ty {
                        Ty::Option | Ty::Vec => (),
                        _ => ty = Ty::Other,
                    }
                }
                match ty {
                    Ty::Bool => {
                        if res.has_method("default_value") {
                            panic!("default_value is meaningless for bool")
                        }
                        if res.has_method("required") {
                            panic!("required is meaningless for bool")
                        }
                    }
                    Ty::Option => {
                        if res.has_method("default_value") {
                            panic!("default_value is meaningless for Option")
                        }
                        if res.has_method("required") {
                            panic!("required is meaningless for Option")
                        }
                    }
                    Ty::OptionOption => {
                        // If it's a positional argument.
                        if !(res.has_method("long") || res.has_method("short")) {
                            panic!("Option<Option<T>> type is meaningless for positional argument")
                        }
                    }
                    Ty::OptionVec => {
                        // If it's a positional argument.
                        if !(res.has_method("long") || res.has_method("short")) {
                            panic!("Option<Vec<T>> type is meaningless for positional argument")
                        }
                    }

                    _ => (),
                }
                res.kind = Kind::Arg(ty);
            }
        }

        res
    }
    fn set_kind(&mut self, kind: Kind) {
        if let Kind::Arg(_) = self.kind {
            self.kind = kind;
        } else {
            panic!("subcommands cannot be flattened");
        }
    }
    pub fn has_method(&self, method: &str) -> bool {
        self.methods.iter().find(|m| m.name == method).is_some()
    }
    pub fn methods(&self) -> proc_macro2::TokenStream {
        let methods = self.methods.iter().map(|&Method { ref name, ref args }| {
            let name = syn::Ident::new(&name, proc_macro2::Span::call_site());
            if name == "short" {
                quote!( .#name(#args.chars().nth(0).unwrap()) )
            } else {
                quote!( .#name(#args) )
            }
        });
        quote!( #(#methods)* )
    }
    pub fn cased_name(&self) -> &str { &self.cased_name }
    pub fn parser(&self) -> &(Parser, proc_macro2::TokenStream) { &self.parser }
    pub fn kind(&self) -> Kind { self.kind }
    pub fn casing(&self) -> CasingStyle { self.casing }
}
