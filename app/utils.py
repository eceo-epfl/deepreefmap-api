from fastapi import HTTPException
import base64


def decode_base64(value: str) -> tuple[bytes, str]:
    """Decode base64 string to csv bytes"""
    # Split the string using the comma as a delimiter
    data_parts = value.split(",")

    # Extract the data type and base64-encoded content
    if "text/csv" in data_parts[0]:
        type = "csv"
    elif "gpx" in data_parts[0]:
        type = "gpx"
    else:
        raise HTTPException(
            status_code=400,
            detail="Only CSV and GPX files are supported",
        )

    base64_content = data_parts[1]
    rawdata = base64.b64decode(base64_content)

    return rawdata, type
