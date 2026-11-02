//! Protocollo ASCII TLX: parte pura, senza I/O, interamente testata.

pub mod bcast;
pub mod command;
pub mod danger;
pub mod info;
pub mod response;
pub mod routing;

pub use bcast::{encode_17564, parse_17564, BcastAssembler, BcastBlock, BcastError};
pub use command::{
    ci, decode_macro, di, di_all, do_, do_all, macro_, macro_chunks, parse_command, si, so,
    CmdError, DecodedMacro, MacroOp, MatrixError, ParsedCommand, ALL_PORTS, MACRO_MAX_PAIRS,
};
pub use danger::is_dangerous;
pub use info::{
    decode_options, parse_xalarm, parse_xinstalled, parse_xlastevent, parse_xlos, parse_xmaxcards,
    parse_xmaxports, parse_xoptions, parse_xportconfig, parse_xrxpwr, parse_xtxpwr, parse_xtype,
    parse_xversion, MatrixInfo, OptionsBits, PortConfig,
};
pub use response::{error_description, parse_response, ParseError, Response};
pub use routing::Routing;
