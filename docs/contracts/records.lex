Record Client Contract

    `postel-bucket` provides exact object reads, create-only writes and prefix
    listings over an injected `ObjectStore`. The client supplies storage
    behavior; callers own record schemas and path conventions.

1. Paths

    `RecordPath` accepts a non-empty relative path. It refuses leading or
    trailing slashes, empty segments, `.` segments and `..` segments. The same
    value addresses create and get operations and supplies the prefix for a
    listing.

    The client treats a path as an opaque address after validation. It does not
    parse record fields from path segments or require dates, schema versions or
    a particular namespace layout.

2. Creation

    `BucketClient::create` creates one immutable object at one path. It refuses
    an existing path with `BucketError::AlreadyExists` and never overwrites the
    previous content. The interface exposes no update or delete operation.

3. Reading and Listing

    `BucketClient::get` returns the bytes at one exact path. `BucketClient::list`
    returns validated paths under one prefix in sorted order. The client does
    not filter on record contents or compute cross-record answers.

4. Adapters and Errors

    `BucketClient::from_store` accepts any `ObjectStore` implementation.
    `from_gcs_env` constructs the included Google Cloud Storage adapter from
    environment configuration. Vendor errors become `BucketError` values, and
    no Google Cloud Storage type appears in the record operations.
