package com.bowline.billing.web;

import com.bowline.billing.storage.StorageException;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.validation.ConstraintViolationException;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.regex.Pattern;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.dao.DataAccessException;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import org.springframework.http.converter.HttpMessageNotReadableException;
import org.springframework.validation.FieldError;
import org.springframework.web.HttpMediaTypeNotSupportedException;
import org.springframework.web.HttpRequestMethodNotSupportedException;
import org.springframework.web.bind.MethodArgumentNotValidException;
import org.springframework.web.bind.MissingServletRequestParameterException;
import org.springframework.web.bind.annotation.ExceptionHandler;
import org.springframework.web.bind.annotation.RestControllerAdvice;
import org.springframework.web.method.annotation.HandlerMethodValidationException;
import org.springframework.web.method.annotation.MethodArgumentTypeMismatchException;
import org.springframework.web.servlet.NoHandlerFoundException;
import org.springframework.web.servlet.resource.NoResourceFoundException;

/**
 * Turns every failure into an RFC 7807 problem document with a stable {@code code}.
 * Codes follow the API's vocabulary: {@code validation_failed}, {@code not_found},
 * {@code internal} and so on.
 */
@RestControllerAdvice
public class ApiExceptionHandler {

    private static final Logger log = LoggerFactory.getLogger(ApiExceptionHandler.class);

    @ExceptionHandler(MethodArgumentNotValidException.class)
    public ResponseEntity<ProblemResponse> onBodyValidation(MethodArgumentNotValidException ex, HttpServletRequest req) {
        List<ProblemResponse.FieldError> errors = new ArrayList<>();
        for (FieldError fe : ex.getBindingResult().getFieldErrors()) {
            errors.add(new ProblemResponse.FieldError(snakeCase(fe.getField()), messageOf(fe)));
        }
        ex.getBindingResult().getGlobalErrors()
                .forEach(ge -> errors.add(new ProblemResponse.FieldError(ge.getObjectName(), ge.getDefaultMessage())));
        errors.sort(Comparator.comparing(ProblemResponse.FieldError::field));
        return validation("Request body failed validation.", req, errors);
    }

    @ExceptionHandler(InvalidRequestException.class)
    public ResponseEntity<ProblemResponse> onInvalidRequest(InvalidRequestException ex, HttpServletRequest req) {
        return validation(ex.getMessage(), req, ex.errors());
    }

    @ExceptionHandler(ConstraintViolationException.class)
    public ResponseEntity<ProblemResponse> onConstraintViolation(ConstraintViolationException ex, HttpServletRequest req) {
        List<ProblemResponse.FieldError> errors = ex.getConstraintViolations().stream()
                .map(v -> new ProblemResponse.FieldError(String.valueOf(v.getPropertyPath()), v.getMessage()))
                .sorted(Comparator.comparing(ProblemResponse.FieldError::field))
                .toList();
        return validation("Request failed validation.", req, errors);
    }

    @ExceptionHandler(HandlerMethodValidationException.class)
    public ResponseEntity<ProblemResponse> onHandlerValidation(HandlerMethodValidationException ex, HttpServletRequest req) {
        List<ProblemResponse.FieldError> errors = ex.getAllValidationResults().stream()
                .flatMap(r -> r.getResolvableErrors().stream()
                        .map(e -> new ProblemResponse.FieldError(r.getMethodParameter().getParameterName(), e.getDefaultMessage())))
                .toList();
        return validation("Request parameters failed validation.", req, errors);
    }

    @ExceptionHandler(MethodArgumentTypeMismatchException.class)
    public ResponseEntity<ProblemResponse> onTypeMismatch(MethodArgumentTypeMismatchException ex, HttpServletRequest req) {
        String expected = ex.getRequiredType() == null ? "a different type" : ex.getRequiredType().getSimpleName();
        return validation("Request parameters failed validation.", req,
                List.of(new ProblemResponse.FieldError(ex.getName(), "must be a valid " + expected)));
    }

    @ExceptionHandler(MissingServletRequestParameterException.class)
    public ResponseEntity<ProblemResponse> onMissingParameter(MissingServletRequestParameterException ex, HttpServletRequest req) {
        return validation("Request parameters failed validation.", req,
                List.of(new ProblemResponse.FieldError(ex.getParameterName(), "is required")));
    }

    @ExceptionHandler(HttpMessageNotReadableException.class)
    public ResponseEntity<ProblemResponse> onUnreadable(HttpMessageNotReadableException ex, HttpServletRequest req) {
        return problem(HttpStatus.BAD_REQUEST, "malformed_request", "Request body is not valid JSON for this endpoint.", req);
    }

    @ExceptionHandler(HttpMediaTypeNotSupportedException.class)
    public ResponseEntity<ProblemResponse> onMediaType(HttpMediaTypeNotSupportedException ex, HttpServletRequest req) {
        return problem(HttpStatus.UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "Send application/json.", req);
    }

    @ExceptionHandler(HttpRequestMethodNotSupportedException.class)
    public ResponseEntity<ProblemResponse> onMethod(HttpRequestMethodNotSupportedException ex, HttpServletRequest req) {
        return problem(HttpStatus.METHOD_NOT_ALLOWED, "method_not_allowed", ex.getMessage(), req);
    }

    @ExceptionHandler({NoResourceFoundException.class, NoHandlerFoundException.class})
    public ResponseEntity<ProblemResponse> onNoRoute(Exception ex, HttpServletRequest req) {
        return problem(HttpStatus.NOT_FOUND, "not_found", "No such route.", req);
    }

    @ExceptionHandler(NotFoundException.class)
    public ResponseEntity<ProblemResponse> onNotFound(NotFoundException ex, HttpServletRequest req) {
        return problem(HttpStatus.NOT_FOUND, "not_found", ex.getMessage(), req);
    }

    @ExceptionHandler(StorageException.class)
    public ResponseEntity<ProblemResponse> onStorage(StorageException ex, HttpServletRequest req) {
        log.error("storage failure: {}", ex.getMessage(), ex);
        return problem(HttpStatus.BAD_GATEWAY, "storage_unavailable", "The rendered document could not be stored.", req);
    }

    @ExceptionHandler(DataAccessException.class)
    public ResponseEntity<ProblemResponse> onDatabase(DataAccessException ex, HttpServletRequest req) {
        log.error("database failure: {}", ex.getMostSpecificCause().getMessage(), ex);
        return problem(HttpStatus.SERVICE_UNAVAILABLE, "database_unavailable", "The database is not reachable.", req);
    }

    @ExceptionHandler(Exception.class)
    public ResponseEntity<ProblemResponse> onUnexpected(Exception ex, HttpServletRequest req) {
        log.error("unhandled failure on {} {}", req.getMethod(), req.getRequestURI(), ex);
        return problem(HttpStatus.INTERNAL_SERVER_ERROR, "internal", "Unexpected error.", req);
    }

    private static ResponseEntity<ProblemResponse> validation(
            String detail, HttpServletRequest req, List<ProblemResponse.FieldError> errors) {
        ProblemResponse body = ProblemResponse.validation(detail, RequestIdFilter.requestId(req), errors);
        return ResponseEntity.status(HttpStatus.UNPROCESSABLE_ENTITY)
                .contentType(MediaType.APPLICATION_PROBLEM_JSON)
                .body(body);
    }

    private static ResponseEntity<ProblemResponse> problem(
            HttpStatus status, String code, String detail, HttpServletRequest req) {
        ProblemResponse body = ProblemResponse.of(status, code, detail, RequestIdFilter.requestId(req));
        return ResponseEntity.status(status).contentType(MediaType.APPLICATION_PROBLEM_JSON).body(body);
    }

    private static String messageOf(FieldError fe) {
        String message = fe.getDefaultMessage();
        return message == null || message.isBlank() ? "is invalid" : message;
    }

    /** {@code lines[1].unitPrice -> lines[1].unit_price}, matching the wire format. */
    static String snakeCase(String javaPath) {
        return CAMEL_BOUNDARY.matcher(javaPath).replaceAll("$1_$2").toLowerCase(Locale.ROOT);
    }

    private static final Pattern CAMEL_BOUNDARY = Pattern.compile("([a-z0-9])([A-Z])");
}
