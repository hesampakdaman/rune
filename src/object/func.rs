use crate::arena::{Arena, Block, GcCell, RootObj};
use crate::error::Error;
use crate::lcell::LCellOwner;
use crate::object::{Function, IntoObject, Object};
use crate::opcode::CodeVec;
use crate::opcode::OpCode;
use std::fmt;

use anyhow::{bail, Result};

/// Argument requirments to a function.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub(crate) struct FnArgs {
    /// a &rest argument.
    pub(crate) rest: bool,
    /// minimum required arguments.
    pub(crate) required: u16,
    /// &optional arguments.
    pub(crate) optional: u16,
    /// If this function is advised.
    pub(crate) advice: bool,
}

/// Represents the body of a function that has been byte compiled. Note that
/// this can represent any top level expression, not just functions.
#[derive(Debug, PartialEq)]
pub(crate) struct Expression<'ob> {
    pub(crate) op_codes: CodeVec,
    pub(crate) constants: Vec<Object<'ob>>,
}

/// A function implemented in lisp. Note that all functions are byte compiled,
/// so this contains the byte-code representation of the function.
#[derive(Debug, PartialEq)]
pub(crate) struct LispFn<'ob> {
    pub(crate) body: Expression<'ob>,
    pub(crate) args: FnArgs,
}

impl FnArgs {
    /// Number of arguments needed to fill out the remaining slots on the stack.
    /// If a function has 3 required args and 2 optional, and it is called with
    /// 4 arguments, then 1 will be returned. Indicating that 1 additional `nil`
    /// argument should be added to the stack.
    pub(crate) fn num_of_fill_args(self, args: u16) -> Result<u16> {
        if args < self.required {
            bail!(Error::ArgCount(self.required, args));
        }
        let total = self.required + self.optional;
        if !self.rest && (args > total) {
            bail!(Error::ArgCount(total, args));
        }
        Ok(total.saturating_sub(args))
    }
}

define_unbox!(LispFn, Func, &'ob LispFn<'ob>);

impl<'ob> LispFn<'ob> {
    pub(crate) fn new(
        op_codes: CodeVec,
        constants: Vec<Object<'ob>>,
        required: u16,
        optional: u16,
        rest: bool,
    ) -> Self {
        LispFn {
            body: Expression {
                op_codes,
                constants,
            },
            args: FnArgs {
                required,
                optional,
                rest,
                advice: false,
            },
        }
    }
}

impl<'ob> IntoObject<'ob, Object<'ob>> for LispFn<'ob> {
    fn into_obj<const C: bool>(self, arena: &'ob Block<C>) -> Object<'ob> {
        let x: Function = self.into_obj(arena);
        x.into()
    }
}

impl<'old, 'new> LispFn<'old> {
    pub(crate) fn clone_in<const C: bool>(&self, bk: &'new Block<C>) -> LispFn<'new> {
        LispFn {
            body: Expression {
                op_codes: self.body.op_codes.clone(),
                constants: self.body.constants.iter().map(|x| x.clone_in(bk)).collect(),
            },
            args: self.args,
        }
    }
}

impl<'ob> Default for LispFn<'ob> {
    fn default() -> Self {
        LispFn::new(
            vec_into![OpCode::Constant0, OpCode::Ret].into(),
            vec![Object::NIL],
            0,
            0,
            false,
        )
    }
}

pub(crate) type BuiltInFn = for<'ob, 'id> fn(
    &[Object<'ob>],
    &GcCell<'id, crate::data::Environment>,
    &'ob mut Arena,
    &mut LCellOwner<'id>,
) -> Result<Object<'ob>>;

pub(crate) struct SubrFn {
    pub(crate) subr: BuiltInFn,
    pub(crate) args: FnArgs,
    pub(crate) name: &'static str,
}
define_unbox!(SubrFn, Func, &'ob SubrFn);

impl SubrFn {
    pub(crate) fn call<'gc, 'id>(
        &self,
        args: &GcCell<'id, Vec<RootObj>>,
        env: &GcCell<'id, crate::data::Environment>,
        arena: &'gc mut Arena,
        owner: &mut LCellOwner<'id>,
    ) -> Result<Object<'gc>> {
        let slice = {
            let args = args.borrow_mut(owner, arena);
            let arg_cnt = args.len() as u16;
            let fill_args = self.args.num_of_fill_args(arg_cnt)?;
            for _ in 0..fill_args {
                args.push(Object::NIL);
            }
            unsafe { std::mem::transmute::<&[Object], &[Object<'gc>]>(args.as_gc().as_ref()) }
        };
        (self.subr)(slice, env, arena, owner)
    }
}

#[cfg(test)]
pub(crate) fn new_subr(
    name: &'static str,
    subr: BuiltInFn,
    required: u16,
    optional: u16,
    rest: bool,
) -> SubrFn {
    SubrFn {
        name,
        subr,
        args: FnArgs {
            required,
            optional,
            rest,
            advice: false,
        },
    }
}

impl std::fmt::Debug for SubrFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({} -> {:?})", &self.name, self.args)
    }
}

impl PartialEq for SubrFn {
    #[allow(clippy::fn_to_numeric_cast_any)]
    fn eq(&self, other: &Self) -> bool {
        let lhs = self.subr as *const BuiltInFn;
        let rhs = other.subr as *const BuiltInFn;
        lhs == rhs
    }
}

impl<'ob> IntoObject<'ob, Object<'ob>> for SubrFn {
    fn into_obj<const C: bool>(self, arena: &'ob Block<C>) -> Object<'ob> {
        let x: Function = self.into_obj(arena);
        x.into()
    }
}

#[cfg(test)]
mod test {
    use crate::arena::RootSet;

    use super::*;

    #[test]
    fn function() {
        let roots = &RootSet::default();
        let arena = &Arena::new(roots);
        let constant: Object = 1.into_obj(arena);
        let lisp_fn = LispFn::new(vec_into![0, 1, 2].into(), vec![constant], 0, 0, false);
        let obj: Object = lisp_fn.into_obj(arena);
        assert!(matches!(obj, Object::LispFn(_)));
        format!("{}", obj);
        let func = match obj {
            Object::LispFn(x) => x,
            _ => unreachable!("expected lispfn"),
        };
        assert_eq!(func.body.op_codes, vec_into![0, 1, 2].into());
        assert_eq!(func.body.constants, vec![1]);
        assert_eq!(func.args.required, 0);
        assert_eq!(func.args.optional, 0);
        assert!(!func.args.rest);
    }
}
