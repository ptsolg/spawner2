use proc_macro2::{Literal, TokenStream};
use quote::{quote, ToTokens};
use syn::{
    Attribute, Data, DeriveInput, Error, Field, Lit, Meta, MetaList, MetaNameValue, NestedMeta,
};

struct OptKindOpt {
    value_desc: String,
    parser: Option<TokenStream>,
}

enum OptKind {
    Invalid,
    Opt(OptKindOpt),
    Flag,
}

struct Opt<'a> {
    kind: OptKind,
    names: Vec<String>,
    desc: String,
    field: &'a Field,
}

enum OptAttribute<'a> {
    Name(&'a MetaNameValue, String),
    Names(&'a MetaList, Vec<String>),
    Desc(&'a MetaNameValue, String),
    ValueDesc(&'a MetaNameValue, String),
    Parser(&'a MetaNameValue, String),
}

enum OptContainerAttribute {
    Delimeters(String),
    Usage(String),
    DefaultParser(String),
}

struct OptContainer<'a> {
    delimeters: String,
    usage: String,
    default_parser: Option<TokenStream>,
    opts: Vec<Opt<'a>>,
    ast: &'a DeriveInput,
}

impl Default for OptKindOpt {
    fn default() -> Self {
        Self {
            value_desc: String::new(),
            parser: None,
        }
    }
}

impl<'a> OptAttribute<'a> {
    fn names_from_meta_list(list: &'a MetaList) -> Result<Self, Error> {
        let mut names: Vec<String> = Vec::new();
        for item in list.nested.iter() {
            match item {
                NestedMeta::Literal(l) => names.push(expect_str(l)?),
                NestedMeta::Meta(m) => {
                    return Err(Error::new_spanned(m, "Expected string literal"));
                }
            }
        }
        Ok(OptAttribute::Names(list, names))
    }

    fn expected_one_of_err<T: ToTokens>(v: &T) -> Error {
        Error::new_spanned(
            v,
            "Expected one of: name = \"...\", names(...), desc = \"...\", \
             value_desc = \"...\" parser = \"...\"",
        )
    }

    fn from_name_value(nameval: &'a MetaNameValue) -> Result<Self, Error> {
        let lit = &nameval.lit;
        match nameval.ident.to_string().as_str() {
            "name" => Ok(OptAttribute::Name(nameval, expect_str(lit)?)),
            "desc" => Ok(OptAttribute::Desc(nameval, expect_str(lit)?)),
            "value_desc" => Ok(OptAttribute::ValueDesc(nameval, expect_str(lit)?)),
            "parser" => Ok(OptAttribute::Parser(nameval, expect_str(lit)?)),
            _ => Err(OptAttribute::expected_one_of_err(nameval)),
        }
    }

    fn from_meta(meta: &'a Meta) -> Result<Self, Error> {
        match meta {
            Meta::List(list) => {
                if list.ident != "names" {
                    Err(OptAttribute::expected_one_of_err(meta))
                } else {
                    OptAttribute::names_from_meta_list(&list)
                }
            }
            Meta::NameValue(nameval) => OptAttribute::from_name_value(&nameval),
            _ => Err(OptAttribute::expected_one_of_err(meta)),
        }
    }
}

impl<'a> Opt<'a> {
    fn new(kind: OptKind, field: &'a Field) -> Self {
        Opt {
            kind: kind,
            names: Vec::new(),
            desc: String::new(),
            field: field,
        }
    }

    fn from_meta_list(field: &'a Field, list: &MetaList) -> Result<Self, Error> {
        let mut attrs: Vec<OptAttribute> = Vec::new();
        for item in list.nested.iter() {
            match item {
                NestedMeta::Meta(m) => attrs.push(OptAttribute::from_meta(&m)?),
                _ => return Err(OptAttribute::expected_one_of_err(item)),
            }
        }

        let kind = match list.ident.to_string().as_str() {
            "opt" => OptKind::Opt(OptKindOpt::default()),
            "flag" => OptKind::Flag,
            _ => OptKind::Invalid,
        };

        let mut opt = Opt::new(kind, field);
        for attr in &attrs {
            match attr {
                OptAttribute::Name(_, s) => {
                    opt.names = vec![s.clone()];
                }
                OptAttribute::Names(_, v) => {
                    opt.names = v.clone();
                }
                OptAttribute::Desc(_, s) => {
                    opt.desc = s.clone();
                }
                OptAttribute::ValueDesc(nameval, s) => {
                    if let OptKind::Opt(ref mut v) = opt.kind {
                        v.value_desc = s.clone();
                    } else {
                        return Err(Error::new_spanned(
                            nameval,
                            "Value description allowed on options only",
                        ));
                    }
                }
                OptAttribute::Parser(nameval, s) => {
                    if let OptKind::Opt(ref mut v) = opt.kind {
                        v.parser = Some(s.parse().unwrap());
                    } else {
                        return Err(Error::new_spanned(
                            nameval,
                            "Parser allowed on options only",
                        ));
                    }
                }
            }
        }

        if opt.names.len() == 0 {
            return Err(Error::new_spanned(list, "Unnamed options are not allowed"));
        }

        Ok(opt)
    }

    fn from_meta(field: &'a Field, attr: &Attribute, meta: Option<Meta>) -> Result<Self, Error> {
        if let Some(m) = meta {
            if let Meta::List(list) = m {
                return Opt::from_meta_list(field, &list);
            }
        }
        Err(Error::new_spanned(
            attr,
            "Invalid attribute in #[opt(...)] or in #[flag(...)]",
        ))
    }

    fn from_field(field: &'a Field) -> Result<Vec<Self>, Error> {
        let mut opts: Vec<Self> = Vec::new();
        for attr in field.attrs.iter().rev() {
            if attr.path.segments.len() == 1 {
                let ident = &attr.path.segments[0].ident;
                if ident == "opt" || ident == "flag" {
                    opts.push(Opt::from_meta(field, attr, attr.interpret_meta())?);
                }
            }
        }
        if opts.len() == 0 {
            opts.push(Opt::new(OptKind::Invalid, field));
        }
        Ok(opts)
    }
}

impl OptContainerAttribute {
    fn expected_one_of_err<T: ToTokens>(v: &T) -> Error {
        Error::new_spanned(
            v,
            "Expected one of: delimeters = \"...\", usage = \"...\" \
             default_parser = \"...\"",
        )
    }

    fn from_meta(meta: &Meta) -> Result<Self, Error> {
        if let Meta::NameValue(nameval) = meta {
            match nameval.ident.to_string().as_ref() {
                "delimeters" => Ok(OptContainerAttribute::Delimeters(expect_str(&nameval.lit)?)),
                "usage" => Ok(OptContainerAttribute::Usage(expect_str(&nameval.lit)?)),
                "default_parser" => Ok(OptContainerAttribute::DefaultParser(expect_str(
                    &nameval.lit,
                )?)),
                _ => Err(OptContainerAttribute::expected_one_of_err(meta)),
            }
        } else {
            Err(OptContainerAttribute::expected_one_of_err(meta))
        }
    }
}

impl<'a> OptContainer<'a> {
    fn parse_meta_list(list: &MetaList) -> Result<Vec<OptContainerAttribute>, Error> {
        let mut attrs: Vec<OptContainerAttribute> = Vec::new();
        for item in list.nested.iter() {
            match item {
                NestedMeta::Meta(m) => attrs.push(OptContainerAttribute::from_meta(&m)?),
                _ => return Err(OptContainerAttribute::expected_one_of_err(item)),
            }
        }
        Ok(attrs)
    }

    fn parse_meta(
        attr: &Attribute,
        meta: Option<Meta>,
    ) -> Result<Vec<OptContainerAttribute>, Error> {
        if let Some(m) = meta {
            if let Meta::List(list) = m {
                return OptContainer::parse_meta_list(&list);
            }
        }
        Err(Error::new_spanned(
            attr,
            "Invalid attributes in #[optcont(...)]",
        ))
    }

    fn parse_attrs(attrs: &Vec<Attribute>) -> Result<Vec<OptContainerAttribute>, Vec<Error>> {
        let mut errors: Vec<Error> = Vec::new();
        let mut result: Vec<OptContainerAttribute> = Vec::new();
        for attr in attrs.iter() {
            if attr.path.segments.len() == 1 && attr.path.segments[0].ident == "optcont" {
                match OptContainer::parse_meta(&attr, attr.interpret_meta()) {
                    Ok(att) => result.extend(att),
                    Err(e) => errors.push(e),
                }
            }
        }
        match errors.len() {
            0 => Ok(result),
            _ => Err(errors),
        }
    }

    fn init_opts(&mut self) -> Result<(), Vec<Error>> {
        let data = match self.ast.data {
            Data::Struct(ref data) => data,
            Data::Enum(_) => {
                return Err(vec![Error::new(
                    self.ast.ident.span(),
                    "Derive for enums is not supported",
                )]);
            }
            Data::Union(_) => {
                return Err(vec![Error::new(
                    self.ast.ident.span(),
                    "Derive for unions is not supported",
                )]);
            }
        };

        let mut errors: Vec<Error> = Vec::new();
        for field in data.fields.iter() {
            match Opt::from_field(field) {
                Ok(opts) => self.opts.extend(opts),
                Err(e) => errors.push(e),
            }
        }
        match errors.len() {
            0 => Ok(()),
            _ => Err(errors),
        }
    }

    fn init_attrs(&mut self) -> Result<(), Vec<Error>> {
        let mut errors: Vec<Error> = Vec::new();
        match OptContainer::parse_attrs(&self.ast.attrs) {
            Ok(attrs) => {
                for att in attrs {
                    match att {
                        OptContainerAttribute::Delimeters(d) => {
                            self.delimeters = d.clone();
                        }
                        OptContainerAttribute::Usage(u) => {
                            self.usage = u.clone();
                        }
                        OptContainerAttribute::DefaultParser(p) => {
                            self.default_parser = Some(p.parse().unwrap());
                        }
                    }
                }
            }
            Err(e) => errors.extend(e),
        }
        match errors.len() {
            0 => Ok(()),
            _ => Err(errors),
        }
    }

    fn from_ast(ast: &'a DeriveInput) -> Result<Self, Vec<Error>> {
        let mut cont = Self {
            delimeters: String::new(),
            usage: String::new(),
            default_parser: None,
            opts: Vec::new(),
            ast: ast,
        };
        cont.init_opts()?;
        cont.init_attrs()?;
        Ok(cont)
    }

    fn build_help_fn(&self) -> TokenStream {
        let usage = &self.usage;
        let opts = self
            .opts
            .iter()
            .map(|x| opt_help(x, self.delimeters.chars().next().unwrap_or(' ')))
            .collect::<String>();
        quote! {
            fn help() -> String {
                format!(
                    "Usage: {}\nOptions:\n{}\n",
                    #usage,
                    #opts
                )
            }
        }
    }

    fn build_register_opts(&self) -> Vec<TokenStream> {
        self.opts
            .iter()
            .filter_map(|opt| {
                let member_func = match &opt.kind {
                    OptKind::Flag => quote!(flag),
                    OptKind::Opt(_) => quote!(opt),
                    _ => return None,
                };
                let names: Vec<Lit> = opt
                    .names
                    .iter()
                    .map(|name| Lit::new(Literal::string(name)))
                    .collect();
                Some(quote! {
                    parser.#member_func(&[#(#names),*]);
                })
            })
            .collect()
    }

    fn opt_parser<'b>(&'b self, opt: &'b Opt) -> Result<&'b TokenStream, Error> {
        if let OptKind::Opt(ref v) = opt.kind {
            if let Some(parser) = v.parser.as_ref().or(self.default_parser.as_ref()) {
                return Ok(parser);
            }
        }
        Err(Error::new_spanned(
            opt.field,
            "Unable to find parser for this field",
        ))
    }

    fn build_set_opts(&self) -> Result<Vec<TokenStream>, Vec<Error>> {
        let mut set_opts: Vec<TokenStream> = Vec::new();
        let mut errors: Vec<Error> = Vec::new();

        for opt in &self.opts {
            let field = &opt.field.ident;
            let name = Lit::new(Literal::string(
                opt.names.iter().next().unwrap_or(&String::from("")),
            ));
            match opt.kind {
                OptKind::Flag => set_opts.push(quote! {
                    if parser.has_flag(#name) {
                        assert_flag_type_is_bool(&self.#field);
                        self.#field = true;
                    }
                }),
                OptKind::Opt(_) => match self.opt_parser(opt) {
                    Ok(parser) => set_opts.push(quote! {
                        if let Some(entries) = parser.get_opt(#name) {
                            for e in entries {
                                #parser::parse(&mut self.#field, e)?;
                            }
                        }
                    }),
                    Err(e) => errors.push(e),
                },
                _ => {}
            }
        }

        match errors.len() {
            0 => Ok(set_opts),
            _ => Err(errors),
        }
    }

    fn build_parse_fn(&self) -> Result<TokenStream, Vec<Error>> {
        let delimeters = &self.delimeters;
        let register_opts = self.build_register_opts();
        let set_opts = self.build_set_opts()?;

        Ok(quote! {
            fn parse<T, U>(&mut self, argv: T) -> Result<usize, String>
            where
                T: IntoIterator<Item = U>,
                U: AsRef<str>
            {
                use opts::OptParser;
                fn assert_flag_type_is_bool(v: &bool) {}

                let mut parser = OptParser::new(argv, #delimeters);
                #(#register_opts)*
                let parsed_opts = parser.parse();
                #(#set_opts)*
                Ok(parsed_opts)
            }
        })
    }
}

fn opt_help(opt: &Opt, delim: char) -> String {
    let desc_offset = 30;
    let indent = String::from(" ").repeat(desc_offset);

    let mut help = String::from("  ");
    for i in 0..opt.names.len() {
        if i > 0 {
            help.push_str(", ")
        }
        help.push_str(opt.names[i].as_str());
        if let OptKind::Opt(ref v) = opt.kind {
            help.push(delim);
            help.push_str(v.value_desc.as_str());
        }
    }

    let mut is_first = true;
    for line in opt.desc.split("\n") {
        if line.is_empty() {
            continue;
        }
        let help_len = help.len();
        if is_first && help_len < desc_offset {
            help.push_str(String::from(" ").repeat(desc_offset - help_len).as_str());
        } else {
            help.push('\n');
            help.push_str(indent.as_str());
        }
        help.push_str(line);
        is_first = false;
    }
    help.push('\n');
    help
}

fn expect_str(lit: &Lit) -> Result<String, Error> {
    match lit {
        Lit::Str(s) => Ok(s.value()),
        _ => Err(Error::new_spanned(lit, "Expected string literal")),
    }
}

pub fn expand_derive_cmd_line_options(ast: &DeriveInput) -> Result<TokenStream, Vec<Error>> {
    let cont = OptContainer::from_ast(ast)?;
    if let Data::Struct(_) = ast.data {
        let struct_name = &ast.ident;
        let help_fn = cont.build_help_fn();
        let parse_fn = cont.build_parse_fn()?;
        Ok(quote! {
            impl CmdLineOptions for #struct_name {
                #help_fn
                #parse_fn
            }
        })
    } else {
        Err(Vec::new())
    }
}
