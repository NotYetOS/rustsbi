//! 这个模块的两个宏应该公开
//! 如果制造实例的时候，给定了stdout，那么就会打印到这个stdout里面
use embedded_hal::serial::{Read, Write};
use nb::block;
use alloc::vec::Vec;
use alloc::string::String;
use lazy_static::lazy_static;

lazy_static! {
    static ref BYTE_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());
}

lazy_static! {
    static ref CHARS: Mutex<String> = Mutex::new(String::new());
}

fn push_byte(byte: u8) {
    BYTE_BUFFER.lock().push(byte);
}

fn generate_chars() {
    let mut buf_lock = BYTE_BUFFER.lock();
    let mut chars_lock = CHARS.lock();
    let len = buf_lock.len();
    let chars = core::str::from_utf8(&buf_lock[0..len]).unwrap();
    chars_lock.push_str(chars);
    buf_lock.clear();
}

fn chars_len() -> usize {
    CHARS.lock().len()
}

fn getchar_from_chars() -> usize {
    let ch = CHARS.lock().remove(0);
    ch as usize
}

/// Legacy standard input/output
pub trait LegacyStdio: Send {
    /// Get a character from legacy stdin
    fn getchar(&mut self) -> usize;
    /// Put a character into legacy stdout
    fn putchar(&mut self, ch: u8);
}

/// Use serial in `embedded-hal` as legacy standard input/output
struct EmbeddedHalSerial<T> {
    inner: T,
}

impl<T> EmbeddedHalSerial<T> {
    /// Create a wrapper with a value
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Send> LegacyStdio for EmbeddedHalSerial<T>
where
    T: Read<u8> + Write<u8>,
{
    // 反正sbi返回值是usize，这里返回也是usize，何乐而不为（其实只要u32）
    // 通过阻塞获得第一个字符，然后直到读取错误（即读取结束），
    // 获得字符的多少取决于你输入法一次能打多少
    // 通过将其转换成String，一个个remove即可
    // 上可utf8，下可ascii
    // 运行的开销自然多了一些些（总比返回-1，各种特权级乱跳的好）
    fn getchar(&mut self) -> usize {
        if chars_len() != 0 {
            return getchar_from_chars();
        }

        let first_byte = block!(
            self.inner.try_read()
        ).ok().unwrap();

        push_byte(first_byte);

        loop {
            match self.inner.try_read() {
                Ok(byte) => push_byte(byte),
                Err(_) => {
                    generate_chars();
                    break;
                }
            }
        }

        getchar_from_chars()
    }

    fn putchar(&mut self, ch: u8) {
        // 直接调用函数写一个字节
        block!(self.inner.try_write(ch)).ok();
        // 写一次flush一次，因为是legacy，就不考虑效率了
        block!(self.inner.try_flush()).ok();
    }
}

struct Fused<T, R>(T, R);

// 和上面的原理差不多，就是分开了
impl<T, R> LegacyStdio for Fused<T, R>
where
    T: Write<u8> + Send + 'static,
    R: Read<u8> + Send + 'static,
{
    fn getchar(&mut self) -> usize {
        if chars_len() != 0 {
            return getchar_from_chars();
        }

        let first_byte = block!(
            self.1.try_read()
        ).ok().unwrap();
        push_byte(first_byte);

        loop {
            match self.1.try_read() {
                Ok(byte) => push_byte(byte),
                Err(_) => {
                    generate_chars();
                    break;
                }
            }
        }

        getchar_from_chars()
    }

    fn putchar(&mut self, ch: u8) {
        block!(self.0.try_write(ch)).ok();
        block!(self.0.try_flush()).ok();
    }
}

use alloc::boxed::Box;
use spin::Mutex;

lazy_static::lazy_static! {
    static ref LEGACY_STDIO: Mutex<Option<Box<dyn LegacyStdio>>> =
        Mutex::new(None);
}

#[doc(hidden)] // use through a macro
pub fn init_legacy_stdio_embedded_hal<T: Read<u8> + Write<u8> + Send + 'static>(serial: T) {
    let serial = EmbeddedHalSerial::new(serial);
    *LEGACY_STDIO.lock() = Some(Box::new(serial));
}

#[doc(hidden)] // use through a macro
pub fn init_legacy_stdio_embedded_hal_fuse<T, R>(tx: T, rx: R)
where
    T: Write<u8> + Send + 'static,
    R: Read<u8> + Send + 'static,
{
    let serial = Fused(tx, rx);
    *LEGACY_STDIO.lock() = Some(Box::new(serial));
}

pub(crate) fn legacy_stdio_putchar(ch: u8) {
    if let Some(stdio) = LEGACY_STDIO.lock().as_mut() {
        stdio.putchar(ch)
    }
}

pub(crate) fn legacy_stdio_getchar() -> usize {
    if let Some(stdio) = LEGACY_STDIO.lock().as_mut() {
        stdio.getchar()
    } else {
        0 // default: always return 0
    }
}

use core::fmt;

struct Stdout;

impl fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if let Some(stdio) = LEGACY_STDIO.lock().as_mut() {
            for byte in s.as_bytes() {
                stdio.putchar(*byte)
            }
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    Stdout.write_fmt(args).unwrap();
}

/// Prints to the legacy debug console.
///
/// This is only supported when there exists legacy extension; 
/// otherwise platform caller should use an early kernel input/output device
/// declared in platform specific hardware.
#[macro_export(local_inner_macros)]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::legacy_stdio::_print(core::format_args!($($arg)*));
    });
}

/// Prints to the legacy debug console, with a newline.
///
/// This is only supported when there exists legacy extension; 
/// otherwise platform caller should use an early kernel input/output device
/// declared in platform specific hardware.
#[macro_export(local_inner_macros)]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::legacy_stdio::_print(core::format_args!(core::concat!($fmt, "\n") $(, $($arg)+)?));
    }
}
