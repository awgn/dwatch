use std::{iter::Peekable, ops::Range};

#[derive(PartialEq)]
enum State {
    None,
    Space,
    Sign,
    Digit,
}

pub struct RangeParser {
    heuristic: Box<dyn Fn(char) -> bool + 'static>,
}

impl RangeParser {
    pub fn new(h: fn(char) -> bool) -> Self {
        Self {
            heuristic: Box::new(h),
        }
    }

    pub fn get_numeric_ranges(&self, str: &str) -> Vec<Range<usize>> {
        let mut local_vector = Vec::new();

        let mut local_state = State::Space;
        let mut local_point = Range { start: 0, end: 0 };
        let mut local_index = 0;

        let chars: Peekable<std::str::Chars> = str.chars().peekable();

        for c in chars {
            match local_state {
                State::None => {
                    if self.heuristic.as_ref()(c) {
                        local_state = State::Space;
                    }
                }
                State::Space => {
                    if c.is_ascii_digit() {
                        local_state = State::Digit;
                        local_point.start = local_index;
                    } else if c == '-' || c == '+' {
                        local_state = State::Sign;
                        local_point.start = local_index;
                    } else if !self.heuristic.as_ref()(c) {
                        local_state = State::None;
                    }
                }
                State::Sign => {
                    if c.is_ascii_digit() {
                        local_state = State::Digit;
                    } else if c == '-' || c == '+' {
                        local_state = State::Sign;
                        local_point.start = local_index;
                    } else if self.heuristic.as_ref()(c) {
                        local_state = State::Space;
                    } else {
                        local_state = State::None;
                    }
                }
                State::Digit => {
                    if self.heuristic.as_ref()(c) {
                        local_point.end = local_index;
                        local_vector.push(local_point.clone());
                        local_state = State::Space;
                    } else if !c.is_ascii_digit() {
                        local_state = State::None;
                    }
                }
            }
            local_index += 1;
        }

        if local_state == State::Digit {
            local_point.end = local_index;
            local_vector.push(local_point);
        }

        local_vector
    }
}

// test for RangeParser

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_parser() {
        let rp = RangeParser::new(|c| c == ' ' || c == ',' || c == ';');
        let ranges = rp.get_numeric_ranges("1 2 3 4 5 6 7 8 9 10");
        assert_eq!(ranges.len(), 10);
        assert_eq!(ranges[0], Range { start: 0, end: 1 });
        assert_eq!(ranges[1], Range { start: 2, end: 3 });
        assert_eq!(ranges[2], Range { start: 4, end: 5 });
        assert_eq!(ranges[3], Range { start: 6, end: 7 });
        assert_eq!(ranges[4], Range { start: 8, end: 9 });
        assert_eq!(ranges[5], Range { start: 10, end: 11 });
        assert_eq!(ranges[6], Range { start: 12, end: 13 });
        assert_eq!(ranges[7], Range { start: 14, end: 15 });
        assert_eq!(ranges[8], Range { start: 16, end: 17 });
        assert_eq!(ranges[9], Range { start: 18, end: 20 });
    }
}
