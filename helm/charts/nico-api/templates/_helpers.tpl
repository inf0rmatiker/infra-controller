{{/*
Allow the release namespace to be overridden for multi-namespace deployments.
*/}}
{{- define "nico-api.namespace" -}}
{{- default .Release.Namespace .Values.namespaceOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Expand the name of the chart.
*/}}
{{- define "nico-api.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "nico-api.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "nico-api.labels" -}}
helm.sh/chart: {{ include "nico-api.chart" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: site-controller
app.kubernetes.io/name: {{ include "nico-api.name" . }}
app.kubernetes.io/component: api
{{- end }}

{{/*
Selector labels
*/}}
{{- define "nico-api.selectorLabels" -}}
app.kubernetes.io/name: {{ include "nico-api.name" . }}
app.kubernetes.io/component: api
{{- end }}

{{/*
Global image reference
*/}}
{{- define "nico-api.image" -}}
{{ .Values.global.image.repository }}:{{ .Values.global.image.tag }}
{{- end }}

{{/*
Certificate spec
Usage: {{ include "nico-api.certificateSpec" (dict "name" "{{ include "nico-api.name" . }}-certificate" "cert" .Values.certificate "global" .Values.global "namespace" (include "nico-api.namespace" .)) }}
*/}}
{{- define "nico-api.certificateSpec" -}}
duration: {{ .global.certificate.duration }}
renewBefore: {{ .global.certificate.renewBefore }}
commonName: {{ printf "%s.%s.svc.cluster.local" (.cert.serviceName | default .svcName) (.cert.identityNamespace | default .namespace) }}
dnsNames:
{{- if .cert.dnsNames }}
{{- range .cert.dnsNames }}
  - {{ . }}
{{- end }}
{{- else }}
  - {{ printf "%s.%s.svc.cluster.local" (.cert.serviceName | default .svcName) (.cert.identityNamespace | default .namespace) }}
{{- if ne (toString .cert.includeShortDnsName) "false" }}
  - {{ printf "%s.%s" (.cert.serviceName | default .svcName) (.cert.identityNamespace | default .namespace) }}
{{- end }}
{{- range .cert.extraDnsNames | default list }}
  - {{ . }}
{{- end }}
{{- end }}
uris:
{{- if .cert.uris }}
{{- range .cert.uris }}
  - {{ . }}
{{- end }}
{{- else }}
  - {{ printf "spiffe://%s/%s/sa/%s" .global.spiffe.trustDomain (.cert.identityNamespace | default .namespace) (.cert.spiffeServiceName | default .cert.serviceName | default .svcName) }}
{{- range .cert.extraUris | default list }}
  - {{ . }}
{{- end }}
{{- end }}
privateKey:
  algorithm: {{ .global.certificate.privateKey.algorithm }}
  size: {{ .global.certificate.privateKey.size }}
issuerRef:
  kind: {{ .global.certificate.issuerRef.kind }}
  name: {{ .global.certificate.issuerRef.name }}
  group: {{ .global.certificate.issuerRef.group }}
secretName: {{ .name }}
{{- end }}

{{/*
Service monitor spec
Usage: {{ include "nico-api.serviceMonitorSpec" (dict "name" "{{ include "nico-api.name" . }}" "port" "http" "monitor" .Values.serviceMonitor "namespace" "nico-system") }}
*/}}
{{- define "nico-api.serviceMonitorSpec" -}}
endpoints:
  - honorLabels: false
    interval: {{ .monitor.interval }}
    port: {{ .port }}
    scheme: http
    scrapeTimeout: {{ .monitor.scrapeTimeout }}
namespaceSelector:
  matchNames:
    - {{ .namespace }}
selector:
  matchLabels:
    app.kubernetes.io/metrics: {{ .name }}
{{- end }}

{{/*
Render the optional [secrets] TOML block for nico-api when .Values.secrets.enabled.
*/}}
{{- define "nico-api.secretsConfigToml" -}}
{{- if .Values.secrets.enabled }}
[secrets]
backends = [{{ range $i, $b := .Values.secrets.backends }}{{ if $i }}, {{ end }}{{ $b | quote }}{{ end }}]
writer = {{ .Values.secrets.writer | quote }}
{{- if .Values.secrets.importFrom }}
import_from = {{ .Values.secrets.importFrom | quote }}
{{- end }}
{{- if .Values.secrets.importApproach }}
import_approach = {{ .Values.secrets.importApproach | quote }}
{{- end }}

{{- if .Values.secrets.writerRouting }}
[secrets.writer_routing]
{{- range $prefix, $backend := .Values.secrets.writerRouting }}
{{ $prefix | quote }} = {{ $backend | quote }}
{{- end }}
{{- end }}

[secrets.kms]
active = {{ .Values.secrets.kms.active | quote }}

[secrets.kms.providers.{{ .Values.secrets.kms.active }}]
type = {{ .Values.secrets.kms.provider.type | quote }}
{{- range $kekId, $source := .Values.secrets.kms.provider.keys }}
keys.{{ $kekId }} = { {{ if $source.env }}env = {{ $source.env | quote }}{{ else if $source.file }}file = {{ $source.file | quote }}{{ end }} }
{{- end }}

[secrets.routing]
{{- range $prefix, $kekId := .Values.secrets.routing }}
{{ $prefix | quote }} = {{ $kekId | quote }}
{{- end }}
{{- end }}
{{- end }}
