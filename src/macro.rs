#[macro_export]
macro_rules! note {
    ($fmt:literal) => {
        println!("{}", format!($fmt));
    };
    ($fmt:literal, $($code:tt)+) => {
        print!("{}", format!($fmt, $($code)*));
    };
}

#[macro_export]
macro_rules! say {
    ($fmt:literal) => {
        println!("{}", format!($fmt));
    };
    ($fmt:literal, $($code:tt)*) => {
        println!("{}", format!($fmt, $($code)*));
    };
}
