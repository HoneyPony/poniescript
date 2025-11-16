enum IrExpr {
    Binary,
    Block,
    If,
    IfElse,
    Call,
    Variable,
}

struct IrFun {
    value: IrExpr,
}

struct Niche {

}

enum IrTypeKind {
    Primitive,
    Pointer(Box<IrType>),
    Struct(Vec<IrType>),
    TaggedUnion {
        variants: Vec<IrType>,
        consumed_niches: Vec<Niche>,
    }
}

struct IrType {

}