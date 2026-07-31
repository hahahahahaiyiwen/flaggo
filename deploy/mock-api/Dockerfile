FROM python:3.12-slim

WORKDIR /workspace

COPY contracts/conformance/requirements.txt contracts/conformance/requirements.txt
RUN python -m pip install --no-cache-dir -r contracts/conformance/requirements.txt

COPY contracts contracts
COPY tools tools

ENV FLAGGO_HOST=0.0.0.0
ENV FLAGGO_PORT=8080

EXPOSE 8080

CMD ["python", "tools/dev.py", "serve"]
