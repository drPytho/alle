use serde::{Deserialize, Serialize};
use std::fmt;

// const char *
// quote_identifier(const char *ident)
// {
// 	/*
// 	 * Can avoid quoting if ident starts with a lowercase letter or underscore
// 	 * and contains only lowercase letters, digits, and underscores, *and* is
// 	 * not any SQL keyword.  Otherwise, supply quotes.
// 	 */
// 	int			nquotes = 0;
// 	bool		safe;
// 	const char *ptr;
// 	char	   *result;
// 	char	   *optr;
//
// 	/*
// 	 * would like to use <ctype.h> macros here, but they might yield unwanted
// 	 * locale-specific results...
// 	 */
// 	safe = ((ident[0] >= 'a' && ident[0] <= 'z') || ident[0] == '_');
//
// 	for (ptr = ident; *ptr; ptr++)
// 	{
// 		char		ch = *ptr;
//
// 		if ((ch >= 'a' && ch <= 'z') ||
// 			(ch >= '0' && ch <= '9') ||
// 			(ch == '_'))
// 		{
// 			/* okay */
// 		}
// 		else
// 		{
// 			safe = false;
// 			if (ch == '"')
// 				nquotes++;
// 		}
// 	}
//
// 	if (quote_all_identifiers)
// 		safe = false;
//
// 	if (safe)
// 	{
// 		/*
// 		 * Check for keyword.  We quote keywords except for unreserved ones.
// 		 * (In some cases we could avoid quoting a col_name or type_func_name
// 		 * keyword, but it seems much harder than it's worth to tell that.)
// 		 *
// 		 * Note: ScanKeywordLookup() does case-insensitive comparison, but
// 		 * that's fine, since we already know we have all-lower-case.
// 		 */
// 		int			kwnum = ScanKeywordLookup(ident, &ScanKeywords);
//
// 		if (kwnum >= 0 && ScanKeywordCategories[kwnum] != UNRESERVED_KEYWORD)
// 			safe = false;
// 	}
//
// 	if (safe)
// 		return ident;			/* no change needed */
//
// 	result = (char *) palloc(strlen(ident) + nquotes + 2 + 1);
//
// 	optr = result;
// 	*optr++ = '"';
// 	for (ptr = ident; *ptr; ptr++)
// 	{
// 		char		ch = *ptr;
//
// 		if (ch == '"')
// 			*optr++ = '"';
// 		*optr++ = ch;
// 	}
// 	*optr++ = '"';
// 	*optr = '\0';
//
// 	return result;
// }
//

/// A validated, lowercase PostgreSQL channel name
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelName(String);

impl ChannelName {
    /// Create a new ChannelName, validating and converting to lowercase
    pub fn new(name: impl AsRef<str>) -> Result<Self, ChannelNameError> {
        let name = name.as_ref().to_lowercase();
        Self::validate(&name)?;
        Ok(Self(name))
    }

    /// Validate channel name rules
    fn validate(name: &str) -> Result<(), ChannelNameError> {
        if name.is_empty() {
            return Err(ChannelNameError::Empty);
        }

        // PostgreSQL identifiers have a max byte length of 63 bytes
        if name.len() > 63 {
            return Err(ChannelNameError::TooLong);
        }

        Ok(())
    }

    /// Check if the identifier is "safe" (doesn't need quoting)
    /// Safe identifiers start with lowercase letter or underscore and contain only
    /// lowercase letters, digits, and underscores
    fn is_safe(name: &str) -> bool {
        if name.is_empty() {
            return false;
        }

        // Must start with lowercase letter or underscore
        let first_char = name.chars().next().unwrap();
        if !(first_char.is_ascii_lowercase() || first_char == '_') {
            return false;
        }

        // Can only contain lowercase letters, digits, and underscores
        for ch in name.chars() {
            if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_') {
                return false;
            }
        }

        true
    }

    /// Quote the identifier for use in SQL, following PostgreSQL's quote_identifier rules
    /// Returns the identifier with double quotes if needed, doubling any internal quotes
    pub fn quote_identifier(&self) -> String {
        // If the name is safe (lowercase, alphanumeric + underscore), return as-is
        if Self::is_safe(&self.0) {
            return self.0.clone();
        }

        // Otherwise, wrap in quotes and double any internal quotes
        let mut result = String::with_capacity(self.0.len() + 2);

        for ch in self.0.chars() {
            if ch == '"' {
                result.push('"'); // Double the quote
            }
            result.push(ch);
        }

        result
    }

    /// Get the channel name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ChannelName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChannelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for ChannelName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChannelName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ChannelName::new(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone)]
pub enum ChannelNameError {
    Empty,
    TooLong,
    InvalidStart,
    InvalidChar,
}

impl fmt::Display for ChannelNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelNameError::Empty => write!(f, "Channel name cannot be empty"),
            ChannelNameError::TooLong => write!(f, "Channel name too long (max 63 characters)"),
            ChannelNameError::InvalidStart => {
                write!(f, "Channel name must start with a letter or underscore")
            }
            ChannelNameError::InvalidChar => write!(
                f,
                "Channel name can only contain letters, numbers, and underscores"
            ),
        }
    }
}

impl std::error::Error for ChannelNameError {}
