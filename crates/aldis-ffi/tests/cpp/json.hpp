// A small JSON reader and writer for the C++ tests of aldis.hpp (CABI-072): enough for the engine
// scenario files of test-vectors/ (objects, arrays, strings without escapes beyond `\"` and `\\`,
// non-negative integers, booleans, null). No exceptions: a malformed file is reported by `parse`
// returning false.

#ifndef ALDIS_TESTS_JSON_HPP
#define ALDIS_TESTS_JSON_HPP

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace tj {

struct Member;

struct Value {
    enum class Kind { Null, Bool, Number, String, Array, Object };

    Kind kind = Kind::Null;
    bool boolean = false;
    std::uint64_t number = 0;
    std::string string;
    std::vector<Value> array;
    // `Member` is completed below: a vector of an incomplete type is valid, a pair of one is not.
    std::vector<Member> object;

    static Value of_bool(bool b);
    static Value of_number(std::uint64_t n);
    static Value of_string(std::string s);
    static Value of_array();
    static Value of_object();

    /// Adds a member to an object and returns the object, for chaining.
    Value& set(std::string key, Value value);
    Value& push(Value value);

    [[nodiscard]] const Value* find(std::string_view key) const;
    [[nodiscard]] bool has(std::string_view key) const { return find(key) != nullptr; }

    /// The member `key`; a missing one is a test failure, reported and fatal.
    [[nodiscard]] const Value& at(std::string_view key) const {
        const Value* v = find(key);
        if (v == nullptr) {
            std::fprintf(stderr, "missing key %.*s\n", static_cast<int>(key.size()), key.data());
            std::exit(2);
        }
        return *v;
    }
    [[nodiscard]] std::uint64_t num() const { return number; }
    [[nodiscard]] std::uint64_t num_or(std::string_view key, std::uint64_t fallback) const {
        const Value* v = find(key);
        return v == nullptr ? fallback : v->number;
    }
    [[nodiscard]] bool flag() const { return boolean; }
    [[nodiscard]] bool flag_or(std::string_view key, bool fallback) const {
        const Value* v = find(key);
        return v == nullptr ? fallback : v->boolean;
    }
    [[nodiscard]] const std::string& str() const { return string; }
    [[nodiscard]] bool is(std::string_view text) const {
        return kind == Kind::String && string == text;
    }
};

struct Member {
    std::string first;
    Value second;
};

inline Value Value::of_bool(bool b) {
    Value v;
    v.kind = Kind::Bool;
    v.boolean = b;
    return v;
}
inline Value Value::of_number(std::uint64_t n) {
    Value v;
    v.kind = Kind::Number;
    v.number = n;
    return v;
}
inline Value Value::of_string(std::string s) {
    Value v;
    v.kind = Kind::String;
    v.string = std::move(s);
    return v;
}
inline Value Value::of_array() {
    Value v;
    v.kind = Kind::Array;
    return v;
}
inline Value Value::of_object() {
    Value v;
    v.kind = Kind::Object;
    return v;
}

inline Value& Value::push(Value value) {
    array.push_back(std::move(value));
    return *this;
}

inline Value& Value::set(std::string key, Value value) {
    object.push_back(Member{std::move(key), std::move(value)});
    return *this;
}

inline const Value* Value::find(std::string_view key) const {
    for (const auto& member : object) {
        if (member.first == key) {
            return &member.second;
        }
    }
    return nullptr;
}

inline bool equal(const Value& a, const Value& b) {
    if (a.kind != b.kind) {
        return false;
    }
    switch (a.kind) {
    case Value::Kind::Null:
        return true;
    case Value::Kind::Bool:
        return a.boolean == b.boolean;
    case Value::Kind::Number:
        return a.number == b.number;
    case Value::Kind::String:
        return a.string == b.string;
    case Value::Kind::Array:
        if (a.array.size() != b.array.size()) {
            return false;
        }
        for (std::size_t i = 0; i < a.array.size(); ++i) {
            if (!equal(a.array[i], b.array[i])) {
                return false;
            }
        }
        return true;
    case Value::Kind::Object:
        if (a.object.size() != b.object.size()) {
            return false;
        }
        for (const auto& member : a.object) {
            const Value* other = b.find(member.first);
            if (other == nullptr || !equal(member.second, *other)) {
                return false;
            }
        }
        return true;
    }
    return false;
}

inline void write(const Value& v, std::string& out) {
    switch (v.kind) {
    case Value::Kind::Null:
        out += "null";
        break;
    case Value::Kind::Bool:
        out += v.boolean ? "true" : "false";
        break;
    case Value::Kind::Number:
        out += std::to_string(v.number);
        break;
    case Value::Kind::String:
        out += '"';
        out += v.string;
        out += '"';
        break;
    case Value::Kind::Array: {
        out += '[';
        bool first = true;
        for (const auto& item : v.array) {
            if (!first) {
                out += ',';
            }
            first = false;
            write(item, out);
        }
        out += ']';
        break;
    }
    case Value::Kind::Object: {
        out += '{';
        bool first = true;
        for (const auto& member : v.object) {
            if (!first) {
                out += ',';
            }
            first = false;
            out += '"';
            out += member.first;
            out += "\":";
            write(member.second, out);
        }
        out += '}';
        break;
    }
    }
}

inline std::string dump(const Value& v) {
    std::string out;
    write(v, out);
    return out;
}

class Parser {
public:
    explicit Parser(std::string_view text) : text_(text) {}

    bool parse(Value& out) {
        skip();
        if (!value(out, 0)) {
            return false;
        }
        skip();
        return pos_ == text_.size();
    }

private:
    static constexpr int kMaxDepth = 64;

    void skip() {
        // bound: advances pos_ over whitespace and ends at the end of the text.
        while (pos_ < text_.size() && (text_[pos_] == ' ' || text_[pos_] == '\n' ||
                                       text_[pos_] == '\t' || text_[pos_] == '\r')) {
            ++pos_;
        }
    }
    bool eat(char c) {
        if (pos_ < text_.size() && text_[pos_] == c) {
            ++pos_;
            return true;
        }
        return false;
    }
    bool word(std::string_view w) {
        if (text_.substr(pos_, w.size()) == w) {
            pos_ += w.size();
            return true;
        }
        return false;
    }
    bool string(std::string& out) {
        if (!eat('"')) {
            return false;
        }
        // bound: advances pos_ and ends at the closing quote or the end of the text.
        while (pos_ < text_.size() && text_[pos_] != '"') {
            char c = text_[pos_];
            if (c == '\\') {
                ++pos_;
                if (pos_ >= text_.size()) {
                    return false;
                }
                c = text_[pos_];
                if (c == 'u') {
                    // The scenario files use \u only in prose; a placeholder keeps the text.
                    if (pos_ + 4 >= text_.size()) {
                        return false;
                    }
                    pos_ += 4;
                    c = '?';
                } else if (c == 'n') {
                    c = '\n';
                } else if (c != '"' && c != '\\' && c != '/') {
                    return false;
                }
            }
            out += c;
            ++pos_;
        }
        return eat('"');
    }
    bool value(Value& out, int depth) {
        if (depth > kMaxDepth || pos_ >= text_.size()) {
            return false;
        }
        const char c = text_[pos_];
        if (c == '{') {
            ++pos_;
            out = Value::of_object();
            skip();
            if (eat('}')) {
                return true;
            }
            // bound: each pass consumes one member; ends at `}` or a parse error.
            while (true) {
                std::string key;
                skip();
                if (!string(key)) {
                    return false;
                }
                skip();
                if (!eat(':')) {
                    return false;
                }
                skip();
                Value member;
                if (!value(member, depth + 1)) {
                    return false;
                }
                out.object.push_back(Member{std::move(key), std::move(member)});
                skip();
                if (eat('}')) {
                    return true;
                }
                if (!eat(',')) {
                    return false;
                }
            }
        }
        if (c == '[') {
            ++pos_;
            out = Value::of_array();
            skip();
            if (eat(']')) {
                return true;
            }
            // bound: each pass consumes one item; ends at `]` or a parse error.
            while (true) {
                skip();
                Value item;
                if (!value(item, depth + 1)) {
                    return false;
                }
                out.array.push_back(std::move(item));
                skip();
                if (eat(']')) {
                    return true;
                }
                if (!eat(',')) {
                    return false;
                }
            }
        }
        if (c == '"') {
            std::string s;
            if (!string(s)) {
                return false;
            }
            out = Value::of_string(std::move(s));
            return true;
        }
        if (word("true")) {
            out = Value::of_bool(true);
            return true;
        }
        if (word("false")) {
            out = Value::of_bool(false);
            return true;
        }
        if (word("null")) {
            out = Value{};
            return true;
        }
        if (c >= '0' && c <= '9') {
            std::uint64_t n = 0;
            // bound: advances pos_ over the digits of one number.
            while (pos_ < text_.size() && text_[pos_] >= '0' && text_[pos_] <= '9') {
                n = n * 10 + static_cast<std::uint64_t>(text_[pos_] - '0');
                ++pos_;
            }
            out = Value::of_number(n);
            return true;
        }
        return false;
    }

    std::string_view text_;
    std::size_t pos_ = 0;
};

inline bool parse_file(const std::string& path, Value& out) {
    std::FILE* f = std::fopen(path.c_str(), "rb");
    if (f == nullptr) {
        return false;
    }
    std::string text;
    char buf[4096];
    std::size_t n = 0;
    // bound: fread returns 0 at the end of the file.
    while ((n = std::fread(buf, 1, sizeof buf, f)) > 0) {
        text.append(buf, n);
    }
    std::fclose(f);
    return Parser(text).parse(out);
}

}  // namespace tj

#endif  // ALDIS_TESTS_JSON_HPP
