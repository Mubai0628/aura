use std::{
    collections::BTreeMap,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{DNS_ERROR_RCODE_NAME_ERROR, DNS_INFO_NO_RECORDS, ERROR_SUCCESS},
    NetworkManagement::Dns::{
        DNS_QUERY_STANDARD, DNS_RECORDA, DNS_RECORDW, DNS_TYPE_A, DNS_TYPE_AAAA, DnsFree,
        DnsFreeRecordList, DnsQuery_W,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DnsAddressRecord {
    pub address: IpAddr,
    pub ttl_seconds: u32,
}

/// Resolves A and AAAA records through the Windows DNS resolver and preserves
/// each record's TTL. The Egress Proxy uses this rather than accepting IPs from
/// an untrusted sandbox process.
pub fn resolve_dns_addresses(host: &str) -> io::Result<Vec<DnsAddressRecord>> {
    let normalized = host.trim_end_matches('.');
    if normalized.is_empty() || normalized.contains(['\0', '/', '\\']) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "DNS host is invalid"));
    }
    if let Ok(address) = normalized.parse::<IpAddr>() {
        return Ok(vec![DnsAddressRecord { address, ttl_seconds: 60 }]);
    }
    let name = normalized.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
    let mut records = BTreeMap::<IpAddr, u32>::new();
    let mut first_error = None;
    for record_type in [DNS_TYPE_A, DNS_TYPE_AAAA] {
        match query_record_type(&name, record_type) {
            Ok(values) => {
                for value in values {
                    records
                        .entry(value.address)
                        .and_modify(|ttl| *ttl = (*ttl).min(value.ttl_seconds))
                        .or_insert(value.ttl_seconds);
                }
            }
            Err(error) if is_no_record_error(&error) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if records.is_empty() {
        return Err(first_error.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "DNS returned no A or AAAA records")
        }));
    }
    Ok(records
        .into_iter()
        .map(|(address, ttl_seconds)| DnsAddressRecord { address, ttl_seconds })
        .collect())
}

fn query_record_type(name: &[u16], record_type: u16) -> io::Result<Vec<DnsAddressRecord>> {
    let mut head: *mut DNS_RECORDW = ptr::null_mut();
    // DnsQuery_W's generated binding uses the ANSI record pointer alias for
    // this output slot even though the returned linked list is DNS_RECORDW.
    // Both pointer representations are identical and the list is freed by
    // DnsFree with DnsFreeRecordList below.
    let status = unsafe {
        DnsQuery_W(
            name.as_ptr(),
            record_type,
            DNS_QUERY_STANDARD,
            ptr::null_mut(),
            (&raw mut head).cast::<*mut DNS_RECORDA>(),
            ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let guard = DnsRecordList(head);
    let mut current = head;
    let mut output = Vec::new();
    while !current.is_null() {
        // SAFETY: each node belongs to the live list returned by DnsQuery_W.
        let record = unsafe { &*current };
        if record.wType == DNS_TYPE_A {
            // SAFETY: wType identifies the active A union field.
            let octets = unsafe { record.Data.A.IpAddress.to_ne_bytes() };
            output.push(DnsAddressRecord {
                address: IpAddr::V4(Ipv4Addr::from(octets)),
                ttl_seconds: record.dwTtl,
            });
        } else if record.wType == DNS_TYPE_AAAA {
            // SAFETY: wType identifies the active AAAA/IP6Byte union fields.
            let octets = unsafe { record.Data.AAAA.Ip6Address.IP6Byte };
            output.push(DnsAddressRecord {
                address: IpAddr::V6(Ipv6Addr::from(octets)),
                ttl_seconds: record.dwTtl,
            });
        }
        current = record.pNext;
    }
    drop(guard);
    Ok(output)
}

fn is_no_record_error(error: &io::Error) -> bool {
    error.raw_os_error().is_some_and(|code| {
        code == DNS_INFO_NO_RECORDS || code == DNS_ERROR_RCODE_NAME_ERROR as i32
    })
}

struct DnsRecordList(*mut DNS_RECORDW);

impl Drop for DnsRecordList {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the list was allocated by DnsQuery_W and is released
            // exactly once with the documented record-list free kind.
            unsafe { DnsFree(self.0.cast(), DnsFreeRecordList) };
        }
    }
}
