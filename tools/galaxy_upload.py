#!/usr/bin/env python3
"""Upload an APK to the Samsung Galaxy Store with the Content Publish API.

    tools/galaxy_upload.py froxel.apk [--notes-file notes.txt] [--submit]

GALAXY_SERVICE_ACCOUNT_ID and GALAXY_PRIVATE_KEY are a Seller Portal service
account with the Publishing scope (Assistance, API Service) and its private
key (the PEM text, or a path to it); GALAXY_CONTENT_ID is the app's content
ID in Seller Portal. Only the standard library and the openssl command are
used: openssl signs the token request.

The script uploads the file, opens an update of the app (contentUpdate, with
the release notes as "New features" from --notes-file), adds the new binary
with the device settings of the current one, and removes the older binaries
from the update. Without --submit the update stays in the Registering state
in Seller Portal. With --submit it is sent for review (contentSubmit); the
app's publication type decides whether it goes live when the review passes.

The API updates only an app that is registered and for sale: the first
version is registered and submitted by hand in Seller Portal. An update
already in review cannot be changed; wait until it is decided.
"""

import argparse
import base64
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

API = "https://devapi.samsungapps.com"


def env(name):
    value = os.environ.get(name, "").strip()
    if not value:
        sys.exit(f"galaxy_upload: {name} is not set")
    return value


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def signed_jwt(account, key_pem, now):
    header = b64url(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    claims = b64url(
        json.dumps({"iss": account, "scopes": ["publishing"], "iat": now, "exp": now + 1200}).encode()
    )
    message = f"{header}.{claims}".encode()
    with tempfile.NamedTemporaryFile("w", suffix=".pem") as key:
        os.chmod(key.name, 0o600)
        key.write(key_pem)
        key.flush()
        signature = subprocess.run(
            ["openssl", "dgst", "-sha256", "-sign", key.name],
            input=message,
            capture_output=True,
            check=True,
        ).stdout
    return f"{header}.{claims}.{b64url(signature)}"


def request(method, url, headers, body=None, content_type="application/json"):
    headers = dict(headers)
    if body is not None:
        headers["Content-Type"] = content_type
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=600) as response:
            text = response.read()
    except urllib.error.HTTPError as err:
        sys.exit(f"galaxy_upload: {method} {url.split('?')[0]}: {err.code} {err.read().decode(errors='replace')}")
    return json.loads(text) if text.strip() else {}


def multipart(fields, file_field, path):
    boundary = uuid.uuid4().hex
    parts = []
    for name, value in fields.items():
        parts.append(
            f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode()
        )
    parts.append(
        (
            f'--{boundary}\r\nContent-Disposition: form-data; name="{file_field}"; '
            f'filename="{path.name}"\r\nContent-Type: application/vnd.android.package-archive\r\n\r\n'
        ).encode()
        + path.read_bytes()
        + b"\r\n"
    )
    parts.append(f"--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


def check(reply, what):
    """The v2 binary calls answer 200 with a resultCode; 0000 is success."""
    code = reply.get("resultCode")
    if code not in (None, "0000"):
        sys.exit(f"galaxy_upload: {what}: {json.dumps(reply)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("apk", type=pathlib.Path)
    parser.add_argument("--notes-file", type=pathlib.Path)
    parser.add_argument("--submit", action="store_true", help="send the update for review")
    args = parser.parse_args()

    account = env("GALAXY_SERVICE_ACCOUNT_ID")
    key = env("GALAXY_PRIVATE_KEY")
    if not key.startswith("-----"):
        key = pathlib.Path(key).read_text()
    content = env("GALAXY_CONTENT_ID")

    jwt = signed_jwt(account, key, int(time.time()))
    token = request("POST", f"{API}/auth/accessToken", {"Authorization": f"Bearer {jwt}"}, b"{}")
    headers = {
        "Authorization": f"Bearer {token['createdItem']['accessToken']}",
        "service-account-id": account,
    }

    info = request("GET", f"{API}/seller/contentInfo?contentId={urllib.parse.quote(content)}", headers)
    info = info[0] if isinstance(info, list) else info
    old = [b["binarySeq"] for b in info.get("binaryList") or [] if b.get("binarySeq")]
    print(f"Galaxy Store: {info.get('appTitle', content)}, status {info.get('contentStatus', '?')}")

    session = request("POST", f"{API}/seller/createUploadSessionId", headers)
    body, form_type = multipart({"sessionId": session["sessionId"]}, "file", args.apk)
    upload = request("POST", session["url"], headers, body, form_type)
    if upload.get("errorCode") or not upload.get("fileKey"):
        sys.exit(f"galaxy_upload: file upload: {json.dumps(upload)}")
    print(f"Galaxy Store: uploaded {args.apk.name} ({upload.get('fileSize', '?')} bytes)")

    # Only the required fields; "null" and left-out fields keep their values.
    update = {
        "contentId": content,
        "defaultLanguageCode": info["defaultLanguageCode"],
        "paid": info.get("paid", "N"),
        "publicationType": info.get("publicationType", "01"),
    }
    notes = args.notes_file.read_text().strip() if args.notes_file else ""
    if notes:
        update["newFeature"] = notes.encode()[:4000].decode(errors="ignore")
    reply = request("POST", f"{API}/seller/contentUpdate", headers, json.dumps(update).encode())
    if reply.get("errorCode") or reply.get("errorMsg"):
        sys.exit(f"galaxy_upload: contentUpdate: {json.dumps(reply)}")

    binary = {"contentId": content, "filekey": upload["fileKey"], "gms": "N"}
    if old:
        binary["binarySeqForDeviceInfo"] = old[-1]
    reply = request("POST", f"{API}/seller/v2/content/binary", headers, json.dumps(binary).encode())
    check(reply, "add binary")
    new = str(reply.get("data", {}).get("binarySeq", ""))
    print(f"Galaxy Store: added binary {new or '?'}")
    for seq in old:
        if str(seq) == new:
            continue
        query = urllib.parse.urlencode({"contentId": content, "binarySeq": seq})
        check(request("DELETE", f"{API}/seller/v2/content/binary?{query}", headers), f"remove binary {seq}")
        print(f"Galaxy Store: removed binary {seq} from the update")

    if args.submit:
        request("POST", f"{API}/seller/contentSubmit", headers, json.dumps({"contentId": content}).encode())
        print("Galaxy Store: submitted for review")
    else:
        print("Galaxy Store: the update is registering in Seller Portal; submit it there")


if __name__ == "__main__":
    main()
