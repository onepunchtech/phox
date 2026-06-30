#include "tree_sitter/parser.h"
#include <stdbool.h>
#include <string.h>

enum TokenType {
  LAYOUT_START,
  LAYOUT_END,
  LAYOUT_SEMICOLON,
};

#define MAX_DEPTH 64

typedef struct {
  int depth;
  int columns[MAX_DEPTH];
} Scanner;

void *tree_sitter_phox_external_scanner_create(void) {
  Scanner *scanner = calloc(1, sizeof(Scanner));
  scanner->depth = 0;
  return scanner;
}

void tree_sitter_phox_external_scanner_destroy(void *payload) {
  free(payload);
}

unsigned tree_sitter_phox_external_scanner_serialize(void *payload,
                                                     char *buffer) {
  Scanner *scanner = (Scanner *)payload;
  unsigned size = 0;
  if (sizeof(int) + scanner->depth * sizeof(int) <=
      TREE_SITTER_SERIALIZATION_BUFFER_SIZE) {
    memcpy(buffer, &scanner->depth, sizeof(int));
    size += sizeof(int);
    memcpy(buffer + size, scanner->columns, scanner->depth * sizeof(int));
    size += scanner->depth * sizeof(int);
  }
  return size;
}

void tree_sitter_phox_external_scanner_deserialize(void *payload,
                                                    const char *buffer,
                                                    unsigned length) {
  Scanner *scanner = (Scanner *)payload;
  scanner->depth = 0;
  if (length >= sizeof(int)) {
    memcpy(&scanner->depth, buffer, sizeof(int));
    unsigned offset = sizeof(int);
    if (scanner->depth > MAX_DEPTH)
      scanner->depth = MAX_DEPTH;
    if (length >= offset + (unsigned)(scanner->depth) * sizeof(int)) {
      memcpy(scanner->columns, buffer + offset,
             scanner->depth * sizeof(int));
    }
  }
}

bool tree_sitter_phox_external_scanner_scan(void *payload, TSLexer *lexer,
                                             const bool *valid_symbols) {
  Scanner *scanner = (Scanner *)payload;

  // LAYOUT_END at EOF
  if (valid_symbols[LAYOUT_END] && scanner->depth > 0 && lexer->eof(lexer)) {
    scanner->depth--;
    lexer->result_symbol = LAYOUT_END;
    return true;
  }

  // LAYOUT_START: begin a new layout block (after 'let' keyword or 'of')
  if (valid_symbols[LAYOUT_START] && scanner->depth < MAX_DEPTH) {
    lexer->mark_end(lexer);
    // Skip whitespace including newlines to find the block column
    while (lexer->lookahead == '\n' || lexer->lookahead == '\r' ||
           lexer->lookahead == ' ' || lexer->lookahead == '\t') {
      lexer->advance(lexer, true);
    }
    // Skip comment lines
    while (lexer->lookahead == '-') {
      lexer->advance(lexer, true);
      if (lexer->lookahead == '-') {
        while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
          lexer->advance(lexer, true);
        }
        while (lexer->lookahead == '\n' || lexer->lookahead == '\r' ||
               lexer->lookahead == ' ' || lexer->lookahead == '\t') {
          lexer->advance(lexer, true);
        }
      } else {
        break;
      }
    }

    if (!lexer->eof(lexer)) {
      int col = (int)lexer->get_column(lexer);
      scanner->columns[scanner->depth] = col;
      scanner->depth++;
      lexer->result_symbol = LAYOUT_START;
      return true;
    }
  }

  // LAYOUT_SEMICOLON or LAYOUT_END: check after newlines
  if (scanner->depth > 0 &&
      (valid_symbols[LAYOUT_SEMICOLON] || valid_symbols[LAYOUT_END])) {
    lexer->mark_end(lexer);

    // We need to see a newline to trigger layout
    bool saw_newline = false;
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
           lexer->lookahead == '\r') {
      lexer->advance(lexer, true);
    }
    if (lexer->lookahead == '\n') {
      saw_newline = true;
      lexer->advance(lexer, true);
      // Skip further whitespace/newlines
      while (lexer->lookahead == '\n' || lexer->lookahead == '\r' ||
             lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        lexer->advance(lexer, true);
      }
      // Skip comment lines
      while (lexer->lookahead == '-') {
        lexer->advance(lexer, true);
        if (lexer->lookahead == '-') {
          while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
            lexer->advance(lexer, true);
          }
          while (lexer->lookahead == '\n' || lexer->lookahead == '\r' ||
                 lexer->lookahead == ' ' || lexer->lookahead == '\t') {
            lexer->advance(lexer, true);
          }
        } else {
          break;
        }
      }
    }

    if (saw_newline && !lexer->eof(lexer)) {
      int col = (int)lexer->get_column(lexer);
      int block_col = scanner->columns[scanner->depth - 1];

      if (col < block_col && valid_symbols[LAYOUT_END]) {
        scanner->depth--;
        lexer->result_symbol = LAYOUT_END;
        return true;
      }

      if (col == block_col && valid_symbols[LAYOUT_SEMICOLON]) {
        lexer->result_symbol = LAYOUT_SEMICOLON;
        return true;
      }
    }

    if (saw_newline && lexer->eof(lexer) && valid_symbols[LAYOUT_END]) {
      scanner->depth--;
      lexer->result_symbol = LAYOUT_END;
      return true;
    }
  }

  return false;
}
