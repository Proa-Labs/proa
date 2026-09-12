// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Prototype component-composition surface for WebMCP metadata.
//!
//! This is intentionally local to the example. It explores two composition
//! rules before either becomes framework API:
//!
//! - a component can contribute complete tools to its parent;
//! - parameter descriptors contribute only inside an active tool scope.

use std::error::Error;
use std::fmt;

/// Whether invoking a tool may complete without a person's final approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirmation {
    Automatic,
    User,
}

/// The operation-level metadata owned by the component that defines a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebMcpTool {
    pub name: &'static str,
    pub description: &'static str,
    pub confirmation: Confirmation,
}

impl WebMcpTool {
    pub const fn new(
        name: &'static str,
        description: &'static str,
        confirmation: Confirmation,
    ) -> Self {
        Self {
            name,
            description,
            confirmation,
        }
    }
}

/// A single property contributed to the active tool's input object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebMcpParameter {
    pub name: &'static str,
    pub description: Option<&'static str>,
    pub required: bool,
}

impl WebMcpParameter {
    pub const fn new(
        name: &'static str,
        description: Option<&'static str>,
        required: bool,
    ) -> Self {
        Self {
            name,
            description,
            required,
        }
    }
}

/// A complete tool after its child parameter contributions have been folded in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredWebMcpTool {
    pub definition: WebMcpTool,
    pub parameters: Vec<WebMcpParameter>,
}

/// Metadata composition is fallible because names form a public agent API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebMcpComposeError {
    DuplicateParameter {
        tool: &'static str,
        parameter: &'static str,
    },
    DuplicateTool(&'static str),
    NestedTool {
        parent: &'static str,
        child: &'static str,
    },
    ParameterOutsideTool(&'static str),
}

impl fmt::Display for WebMcpComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::DuplicateParameter { tool, parameter } => {
                write!(
                    f,
                    "tool {tool:?} contains duplicate parameter {parameter:?}"
                )
            }
            Self::DuplicateTool(name) => write!(f, "duplicate WebMCP tool {name:?}"),
            Self::NestedTool { parent, child } => {
                write!(
                    f,
                    "WebMCP tool {child:?} cannot be nested inside {parent:?}"
                )
            }
            Self::ParameterOutsideTool(name) => {
                write!(f, "WebMCP parameter {name:?} has no enclosing tool")
            }
        }
    }
}

impl Error for WebMcpComposeError {}

/// Components implement this trait to contribute parameters or complete tools.
///
/// Unlike `WebRender`, this does not write bytes. It folds capability metadata
/// into a page-level registry. Parent components expose child tools by calling
/// the child's implementation with the same context.
pub trait WebMcp {
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError>;
}

/// Page-level collector with an optional active operation scope.
#[derive(Debug, Default)]
pub struct WebMcpContext {
    tools: Vec<RegisteredWebMcpTool>,
    active_tool: Option<usize>,
}

impl WebMcpContext {
    pub fn collect<C>(component: &C) -> Result<Vec<RegisteredWebMcpTool>, WebMcpComposeError>
    where
        C: WebMcp + ?Sized,
    {
        let mut cx = Self::default();
        component.describe_webmcp(&mut cx)?;
        Ok(cx.tools)
    }

    /// Open an operation scope and fold the child parameter components into it.
    pub fn tool<C>(
        &mut self,
        definition: WebMcpTool,
        parameters: &C,
    ) -> Result<(), WebMcpComposeError>
    where
        C: WebMcp + ?Sized,
    {
        if let Some(active) = self.active_tool {
            return Err(WebMcpComposeError::NestedTool {
                parent: self.tools[active].definition.name,
                child: definition.name,
            });
        }

        if self
            .tools
            .iter()
            .any(|tool| tool.definition.name == definition.name)
        {
            return Err(WebMcpComposeError::DuplicateTool(definition.name));
        }

        let index = self.tools.len();
        self.tools.push(RegisteredWebMcpTool {
            definition,
            parameters: Vec::new(),
        });
        self.active_tool = Some(index);

        let result = parameters.describe_webmcp(self);
        self.active_tool = None;

        if result.is_err() {
            self.tools.pop();
        }

        result
    }

    fn parameter(&mut self, parameter: WebMcpParameter) -> Result<(), WebMcpComposeError> {
        let Some(active) = self.active_tool else {
            return Err(WebMcpComposeError::ParameterOutsideTool(parameter.name));
        };

        let tool = &mut self.tools[active];
        if tool
            .parameters
            .iter()
            .any(|existing| existing.name == parameter.name)
        {
            return Err(WebMcpComposeError::DuplicateParameter {
                tool: tool.definition.name,
                parameter: parameter.name,
            });
        }

        tool.parameters.push(parameter);
        Ok(())
    }
}

impl WebMcp for WebMcpParameter {
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        cx.parameter(*self)
    }
}

impl<T> WebMcp for [T]
where
    T: WebMcp,
{
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        for child in self {
            child.describe_webmcp(cx)?;
        }
        Ok(())
    }
}

impl<T, const N: usize> WebMcp for [T; N]
where
    T: WebMcp,
{
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        self.as_slice().describe_webmcp(cx)
    }
}

impl<T> WebMcp for Option<T>
where
    T: WebMcp,
{
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        if let Some(child) = self {
            child.describe_webmcp(cx)?;
        }
        Ok(())
    }
}

impl WebMcp for () {
    fn describe_webmcp(&self, _cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        Ok(())
    }
}

macro_rules! impl_webmcp_tuple {
    ($($name:ident),+) => {
        impl<$($name),+> WebMcp for ($($name,)+)
        where
            $($name: WebMcp,)+
        {
            #[allow(non_snake_case)]
            fn describe_webmcp(
                &self,
                cx: &mut WebMcpContext,
            ) -> Result<(), WebMcpComposeError> {
                let ($($name,)+) = self;
                $($name.describe_webmcp(cx)?;)+
                Ok(())
            }
        }
    };
}

impl_webmcp_tuple!(A, B);
impl_webmcp_tuple!(A, B, C);
impl_webmcp_tuple!(A, B, C, D);
