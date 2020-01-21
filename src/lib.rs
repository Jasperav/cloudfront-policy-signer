/*
MIT License

Copyright (c) 2020 Martin Karlsen Jensen

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

use openssl::rsa;
use openssl::sign::Signer;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::base64::encode_block;
use std::fs;
use std::io::Error as SysIOError;
use log::error;

/// Enumeration of all possible errors returned by the crate
#[derive(Debug)]
pub enum Error {
    /// We received an IO error from the operating system. Refer to std::io::Error for more information
    IOError(SysIOError),
    /// The private key was in an unsupported format or somehow malformed. It only accepts keys in PEM-encoded PKCS#1
    PrivateKeyParseError,
    /// The key could not be converted from a `openssl::rsa::Rsa<openssl::pkey::Private>` to a `PKey<Private>`
    PrivateKeyConvertError,
    /// The policy could not be signed. Refer to the error printed out in the logs
    CouldNotSign,
    /// Blanket error for all errors from OpenSSL that should not occur, but can due to it being written in unsafe C. 
    Unknown,
}

/// Returns a canned policy with the specified constraints as an vector of bytes
/// 
/// # Arguments
/// * `resource` - The protected resource eg. https://example.cloudfront.net/flowerpot.png
/// * `expiry` - The time the resource link should expire at
/// 
/// 
fn generate_canned_policy(resource: &str, expiry: u64) -> Vec<u8> {
    format!("{{\"Statement\":[{{\"Resource\":\"{}}}\",\"Condition\":{{\"DateLessThan\":{{\"AWS:EpochTime\":{}}}}}]}}", resource, expiry).into_bytes()
}


/// Reads the contents of a file into memory and returns it as a vector of bytes
/// 
/// # Arguments
/// * `file` - A file containing an RSA private key usually either retrieved in the AWS interface or generated by OpenSSL. The file must be in PEM-encoded PKCS#1
/// 
/// # Note
/// 
/// See the [CloudFront Documentation](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/private-content-trusted-signers.html#private-content-creating-cloudfront-key-pairs) about creating these keypairs
/// 
/// 
fn read_rsa_private_key(file: &str) -> Result<Vec<u8>, Error> {
    fs::read(&file)
        .map_err(|e| {
            error!("Could not read private key from file due to {}", e);
            Error::IOError(e)
        })
}

/// Parses the read bytes into an represntation of a RSA private key appropriate for OpenSSL
/// 
/// # Arguments
/// * `key` - An array of bytes containing a RSA private key part 
/// 
fn parse_rsa_private_key(key: &[u8]) -> Result<PKey<Private>, Error> {
    rsa::Rsa::private_key_from_pem(&key)
        .map_err(|e| {
            error!("Could not parse RSA private key due to {}", e);
            Error::PrivateKeyParseError
        })
        .and_then(|private_key| {
            PKey::from_rsa(private_key)
                .map_err(|e| {
                    error!("Could not convert RSA private key due to {}", e);
                    Error::PrivateKeyConvertError
                })
        })
}

/// Signs the canned policy and returns it as a vector of bytes
/// 
/// # Arguments
/// * `policy` - An array of bytes containing the properly formatted policy
/// * `private_key` - The representation of the RSA private key part
/// 
/// 
fn sign_canned_policy(policy: &[u8], private_key: &PKey<Private>) -> Result<Vec<u8>, Error> {
    Signer::new(MessageDigest::sha1(), &private_key)
        .map_err(|e| {
            error!("Could not create signer due to {}", e);
            Error::Unknown
        })
        .and_then(|mut signer| {
            signer.update(&policy)
                .map_err(|e| {
                    error!("Could not update signer due to {}", e);
                    Error::Unknown
                })
                .and_then(|_| {
                    signer.sign_to_vec()
                        .map_err(|e| {
                            error!("Could not sign due to {}", e);
                            Error::CouldNotSign
                        })
                })
                
        })
}

/// Base64 encode an array of data and use that to create an URL safe string
/// 
/// # Arguments
/// * `bytes` - An array of bytes to be encoded
/// 
/// 
fn encode_signature_url_safe(bytes: &[u8]) -> String {
    encode_block(&bytes)
        .replace("+", "-")
        .replace("=", "_")
        .replace("/", "~")
}

/// Signs a canned policy with the specified path and expiration date and returns it in an URL safe format appropriate for AWS.
/// 
/// 
/// See [CloudFront Documentation](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/private-content-creating-signed-url-canned-policy.html) for more details
/// 
/// # Arguments
/// * `resource` - The protected resource eg. https://example.cloudfront.net/flowerpot.png
/// * `expiry` - Absolute time that the link expires, given in the form of a unix timestamp in UTC
/// * `private_key_location` - Path where the private key file can be found
/// 
/// 
/// # Example
/// ```
/// use cloudfront_url_signer;
/// 
/// fn main() {
///    let resource = "https://example.cloudfront.net/flowerpot.png";
///    let expiry = 1579532331;
///    let certificate_location = "examples/key.pem";
///    let key_pair_id = "APKAIEXAMPLE";
///
///    let signature = cloudfront_url_signer::create_canned_policy_signature(resource, expiry, certificate_location).unwrap();
///
///    println!("Signed URL is {}", format!("{}?Expires={}&Signature={}&Key-Pair-Id={}", resource, expiry, signature, key_pair_id));
///}
/// ```
/// 
pub fn create_canned_policy_signature(resource: &str, expiry: u64, private_key_location: &str) -> Result<String, Error> {
    read_rsa_private_key(private_key_location).and_then(|key| {
        parse_rsa_private_key(&key).and_then(|private_key| {
            sign_canned_policy(&generate_canned_policy(resource, expiry), &private_key).and_then(|signed_policy| {
                Ok(encode_signature_url_safe(&signed_policy))
            })
        })
    })
}