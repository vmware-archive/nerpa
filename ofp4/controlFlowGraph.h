/*
Copyright 2013-present Barefoot Networks, Inc.
          2022 Vmware, Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

// This file has been adapted from the bmv2 control-flow graph code;
// the constraints are different for OF.

#ifndef EXTENSIONS_OFP4_CONTROLFLOWGRAPH_H_
#define EXTENSIONS_OFP4_CONTROLFLOWGRAPH_H_

#include "ir/ir.h"
#include "frontends/p4/typeMap.h"
#include "frontends/common/resolveReferences/referenceMap.h"
#include "lib/ordered_set.h"

namespace OFP4 {

class CFG final : public IHasDbPrint {
 public:
    class Edge;
    class Node;

    class EdgeSet final : public IHasDbPrint {
     public:
        ordered_set<CFG::Edge*> edges;

        EdgeSet() = default;
        explicit EdgeSet(CFG::Edge* edge) { edges.emplace(edge); }
        explicit EdgeSet(const EdgeSet* other) { mergeWith(other); }

        void mergeWith(const EdgeSet* other)
        { edges.insert(other->edges.begin(), other->edges.end()); }
        void dbprint(std::ostream& out) const;
        void emplace(CFG::Edge* edge) { edges.emplace(edge); }
        size_t size() const { return edges.size(); }
        /// Check if this destination appears in this edgeset.
        /// Importantly, a TableNode is a destination if it points to
        /// the same table as an existing destination (pointer equality
        /// is not enough).
        bool isDestination(const CFG::Node* destination) const;
    };

    class Node : public IHasDbPrint {
     protected:
        friend class CFG;

        static unsigned crtId;
        EdgeSet         predecessors;
        explicit Node(cstring name) : id(crtId++), name(name) {}
        Node() : id(crtId++), name("node_" + Util::toString(id)) {}
        virtual ~Node() {}

     public:
        const unsigned id;
        const cstring  name;
        EdgeSet        successors;

        void dbprint(std::ostream& out) const;
        void addPredecessors(const EdgeSet* set);
        template<typename T> bool is() const { return to<T>() != nullptr; }
        template<typename T> T* to() { return dynamic_cast<T*>(this); }
        template<typename T> const T* to() const { return dynamic_cast<const T*>(this); }
        void computeSuccessors();
        cstring toString() const { return name; }
    };

 public:
    class TableNode final : public Node {
     public:
        const IR::P4Table* table;
        const IR::Expression*      invocation;
        explicit TableNode(const IR::P4Table* table, const IR::Expression* invocation)
        : Node(table->controlPlaneName()), table(table), invocation(invocation)
        { CHECK_NULL(table); CHECK_NULL(invocation); }
    };

    class IfNode final : public Node {
     public:
        const IR::IfStatement* statement;
        explicit IfNode(const IR::IfStatement* statement) : statement(statement)
        { CHECK_NULL(statement); }
    };

    class DummyNode final : public Node {
     public:
        explicit DummyNode(cstring name) : Node(name) {}
    };

 protected:
    enum class EdgeType {
        Unconditional,
        True,
        False,
        Label
    };

 public:
    /**
     * A CFG Edge; can be an in-edge or out-edge.
     */
    class Edge final {
     protected:
        EdgeType type;
        Edge(Node* node, EdgeType type, cstring label) : type(type), endpoint(node), label(label) {}

     public:
        /**
         * The destination node of the edge.  The source node is not known by the edge
         */
        Node*    endpoint;
        cstring  label;  // only present if type == Label

        explicit Edge(Node* node) : type(EdgeType::Unconditional), endpoint(node)
        { CHECK_NULL(node); }
        Edge(Node* node, bool b) :
                type(b ? EdgeType::True : EdgeType::False), endpoint(node)
        { CHECK_NULL(node); }
        Edge(Node* node, cstring label) :
                type(EdgeType::Label), endpoint(node), label(label)
        { CHECK_NULL(node); }
        void dbprint(std::ostream& out) const;
        Edge* clone(Node* node) const
        { return new Edge(node, type, label); }
        Node* getNode() { return endpoint; }
        bool  getBool() {
            BUG_CHECK(isBool(), "Edge is not Boolean");
            return type == EdgeType::True;
        }
        bool isBool() const { return type == EdgeType::True || type == EdgeType::False; }
        bool isUnconditional() const { return type == EdgeType::Unconditional; }
    };

 public:
    Node* entryPoint;
    Node* exitPoint;
    const IR::P4Control* container;
    ordered_set<Node*> allNodes;

    CFG() : entryPoint(nullptr), exitPoint(nullptr), container(nullptr) {}
    Node* makeNode(const IR::P4Table* table, const IR::Expression* invocation) {
        auto result = new TableNode(table, invocation);
        allNodes.emplace(result);
        return result;
    }
    Node* makeNode(const IR::IfStatement* statement) {
        auto result = new IfNode(statement);
        allNodes.emplace(result);
        return result;
    }
    Node* makeNode(cstring name) {
        auto result = new DummyNode(name);
        allNodes.emplace(result);
        return result;
    }
    void build(const IR::P4Control* cc,
               P4::ReferenceMap* refMap, P4::TypeMap* typeMap);
    void setEntry(Node* entry) {
        BUG_CHECK(entryPoint == nullptr, "Entry already set");
        entryPoint = entry;
    }
    void dbprint(std::ostream& out, Node* node, std::set<Node*> &done) const;  // helper
    void dbprint(std::ostream& out) const;
    void computeSuccessors()
    { for (auto n : allNodes) n->computeSuccessors(); }
};

}  // namespace OFP4

#endif /* EXTENSIONS_OFP4_CONTROLFLOWGRAPH_H_ */
